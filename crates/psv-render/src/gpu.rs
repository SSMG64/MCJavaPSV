//! The seam between the renderer and a GPU. A [`Backend`] knows how to put an atlas and meshes
//! on a graphics device and draw them; a [`Renderer`] knows which sections to draw, in what
//! order. Everything that decides what appears on screen lives in the renderer, so a new
//! backend, such as a Vita one, only has to draw what it is told to.

use std::collections::HashMap;

use crate::atlas::Atlas;
use crate::camera::Camera;
use crate::math::{Mat4, Vec3, sub};
use crate::mesh::Mesh;
use crate::quad::Layer;
use crate::raster::{Framebuffer, render_layer};

pub trait Backend {
    type Error: std::fmt::Debug;
    /// A mesh on the device. Dropping it frees it.
    type Mesh;

    fn upload_atlas(&mut self, atlas: &Atlas) -> Result<(), Self::Error>;
    fn upload_mesh(&mut self, mesh: &Mesh) -> Result<Self::Mesh, Self::Error>;
    fn begin_frame(&mut self, sky: [u8; 4]);
    /// Draws one layer of a mesh. `mvp` already includes the mesh's position in the world.
    fn draw(&mut self, mesh: &Self::Mesh, layer: Layer, mvp: &Mat4);
    fn end_frame(&mut self) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub sections: usize,
    pub sections_drawn: usize,
    pub draw_calls: usize,
}

struct Section<M> {
    mesh: M,
    origin: Vec3,
    extent: Vec3,
    layers_used: [bool; 3],
}

pub struct Renderer<B: Backend> {
    backend: B,
    sections: HashMap<[i32; 3], Section<B::Mesh>>,
}

impl<B: Backend> Renderer<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            sections: HashMap::new(),
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Puts a section's mesh on the device, replacing whatever was there. A mesh with nothing
    /// to draw removes the section instead.
    pub fn set_section(
        &mut self,
        key: [i32; 3],
        origin: Vec3,
        extent: Vec3,
        mesh: &Mesh,
    ) -> Result<(), B::Error> {
        if mesh.is_empty() {
            self.sections.remove(&key);
            return Ok(());
        }
        let uploaded = self.backend.upload_mesh(mesh)?;
        let layers_used = Layer::ALL.map(|layer| !mesh.layer(layer).is_empty());
        self.sections.insert(
            key,
            Section {
                mesh: uploaded,
                origin,
                extent,
                layers_used,
            },
        );
        Ok(())
    }

    pub fn remove_section(&mut self, key: [i32; 3]) -> bool {
        self.sections.remove(&key).is_some()
    }

    /// Draws everything the camera can see: opaque layers first, then translucent sections from
    /// the farthest to the nearest so blending sees what is behind.
    pub fn draw_frame(&mut self, camera: &Camera, sky: [u8; 4]) -> Result<FrameStats, B::Error> {
        let view_projection = camera.view_projection();
        let frustum = camera.frustum();
        let mut visible: Vec<(&[i32; 3], &Section<B::Mesh>)> = self
            .sections
            .iter()
            .filter(|(_, s)| {
                frustum.intersects_box(
                    s.origin,
                    [
                        s.origin[0] + s.extent[0],
                        s.origin[1] + s.extent[1],
                        s.origin[2] + s.extent[2],
                    ],
                )
            })
            .collect();
        visible.sort_by_key(|(key, _)| **key);

        let distance_squared = |s: &Section<B::Mesh>| {
            let centre = [
                s.origin[0] + s.extent[0] / 2.0,
                s.origin[1] + s.extent[1] / 2.0,
                s.origin[2] + s.extent[2] / 2.0,
            ];
            let d = sub(centre, camera.position);
            d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
        };
        let mut far_to_near = visible.clone();
        far_to_near.sort_by(|a, b| distance_squared(b.1).total_cmp(&distance_squared(a.1)));

        self.backend.begin_frame(sky);
        let mut draw_calls = 0;
        for (layer, order) in [
            (Layer::Solid, &visible),
            (Layer::Cutout, &visible),
            (Layer::Translucent, &far_to_near),
        ] {
            for (_, section) in order {
                if !section.layers_used[layer.index()] {
                    continue;
                }
                let model = Mat4::translation(section.origin);
                self.backend
                    .draw(&section.mesh, layer, &view_projection.mul(&model));
                draw_calls += 1;
            }
        }
        self.backend.end_frame()?;
        Ok(FrameStats {
            sections: self.sections.len(),
            sections_drawn: visible.len(),
            draw_calls,
        })
    }
}

/// Draws with the CPU rasterizer. Meant for tests and tools, not for playing.
pub struct SoftwareBackend {
    pub frame: Framebuffer,
    atlas: Option<Atlas>,
}

impl SoftwareBackend {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            frame: Framebuffer::new(width, height, [0, 0, 0, 255]),
            atlas: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct NoAtlas;

impl Backend for SoftwareBackend {
    type Error = NoAtlas;
    type Mesh = Mesh;

    fn upload_atlas(&mut self, atlas: &Atlas) -> Result<(), NoAtlas> {
        self.atlas = Some(atlas.clone());
        Ok(())
    }

    fn upload_mesh(&mut self, mesh: &Mesh) -> Result<Mesh, NoAtlas> {
        Ok(mesh.clone())
    }

    fn begin_frame(&mut self, sky: [u8; 4]) {
        self.frame = Framebuffer::new(self.frame.width, self.frame.height, sky);
    }

    fn draw(&mut self, mesh: &Mesh, layer: Layer, mvp: &Mat4) {
        if let Some(atlas) = &self.atlas {
            render_layer(&mut self.frame, mesh, layer, atlas, mvp, [0.0; 3]);
        }
    }

    fn end_frame(&mut self) -> Result<(), NoAtlas> {
        self.atlas.as_ref().map(|_| ()).ok_or(NoAtlas)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use psv_pack::texture::FLAG_HAS_TRANSLUCENT;

    use super::*;
    use crate::atlas::AtlasEntry;
    use crate::camera::FlyInput;
    use crate::geometry::Rotation;
    use crate::mesh::{Appearance, AppearanceProvider, BlockView, NoTint, mesh_region};
    use crate::model::{ModelSource, resolve};
    use crate::quad::bake;

    struct Models(HashMap<String, String>);

    impl ModelSource for Models {
        fn load(&mut self, location: &str) -> Option<String> {
            self.0.get(location).cloned()
        }
    }

    struct One(Appearance);

    impl AppearanceProvider for One {
        fn appearance(&self, state: u32) -> Option<&Appearance> {
            (state == 1).then_some(&self.0)
        }
    }

    struct Block;

    impl BlockView for Block {
        fn state(&self, x: i32, y: i32, z: i32) -> u32 {
            u32::from([x, y, z] == [0, 0, 0])
        }
    }

    fn texture(name: &str, flags: u8, color: [u8; 4]) -> AtlasEntry {
        AtlasEntry {
            name: name.into(),
            width: 4,
            height: 4,
            rgba: color.repeat(16),
            flags,
        }
    }

    fn cube_mesh(atlas: &Atlas, name: &str) -> Mesh {
        let faces: Vec<String> = ["down", "up", "north", "south", "west", "east"]
            .iter()
            .map(|d| format!(r#""{d}":{{"texture":"{name}","cullface":"{d}"}}"#))
            .collect();
        let json = format!(
            r#"{{"elements":[{{"from":[0,0,0],"to":[16,16,16],"faces":{{{}}}}}]}}"#,
            faces.join(",")
        );
        let mut source = Models([("minecraft:m".to_owned(), json)].into());
        let model = resolve("m", &mut source).unwrap();
        let (quads, report) = bake(&model, Rotation::NONE, atlas);
        assert!(report.missing_sprites.is_empty());
        let provider = One(Appearance::new(quads));
        mesh_region(&Block, &provider, &NoTint, [0, 0, 0], [1, 1, 1])
    }

    #[derive(Default)]
    struct Log {
        events: RefCell<Vec<String>>,
        freed: Cell<usize>,
    }

    struct Handle {
        id: usize,
        log: Rc<Log>,
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            self.log.freed.set(self.log.freed.get() + 1);
        }
    }

    struct Recorder {
        log: Rc<Log>,
        next: usize,
    }

    impl Backend for Recorder {
        type Error = ();
        type Mesh = Handle;

        fn upload_atlas(&mut self, _: &Atlas) -> Result<(), ()> {
            Ok(())
        }

        fn upload_mesh(&mut self, _: &Mesh) -> Result<Handle, ()> {
            self.next += 1;
            Ok(Handle {
                id: self.next,
                log: Rc::clone(&self.log),
            })
        }

        fn begin_frame(&mut self, _: [u8; 4]) {
            self.log.events.borrow_mut().push("begin".into());
        }

        fn draw(&mut self, mesh: &Handle, layer: Layer, _: &Mat4) {
            self.log
                .events
                .borrow_mut()
                .push(format!("{layer:?}:{}", mesh.id));
        }

        fn end_frame(&mut self) -> Result<(), ()> {
            self.log.events.borrow_mut().push("end".into());
            Ok(())
        }
    }

    fn recorder() -> (Renderer<Recorder>, Rc<Log>) {
        let log = Rc::new(Log::default());
        (
            Renderer::new(Recorder {
                log: Rc::clone(&log),
                next: 0,
            }),
            log,
        )
    }

    fn atlas() -> Atlas {
        Atlas::build(
            vec![
                texture("minecraft:solid", 0, [200, 50, 50, 255]),
                texture("minecraft:water", FLAG_HAS_TRANSLUCENT, [50, 50, 220, 128]),
            ],
            1,
            64,
        )
        .unwrap()
    }

    fn looking_north(position: Vec3) -> Camera {
        let mut camera = Camera::new(position, 1.0);
        camera.yaw = 180.0;
        camera
    }

    #[test]
    fn only_sections_in_view_are_drawn() {
        let atlas = atlas();
        let mesh = cube_mesh(&atlas, "solid");
        let (mut renderer, log) = recorder();
        let extent = [16.0; 3];
        renderer
            .set_section([0, 0, -1], [0.0, 0.0, -16.0], extent, &mesh)
            .unwrap();
        renderer
            .set_section([0, 0, 1], [0.0, 0.0, 16.0], extent, &mesh)
            .unwrap();
        renderer
            .set_section([9, 0, -1], [144.0, 0.0, -16.0], extent, &mesh)
            .unwrap();

        let stats = renderer
            .draw_frame(&looking_north([8.0, 8.0, 8.0]), [0; 4])
            .unwrap();
        assert_eq!(
            stats,
            FrameStats {
                sections: 3,
                sections_drawn: 1,
                draw_calls: 1
            }
        );
        assert_eq!(log.events.borrow().as_slice(), ["begin", "Solid:1", "end"]);
    }

    #[test]
    fn opaque_layers_come_before_translucent_and_translucent_goes_far_to_near() {
        let atlas = atlas();
        let water = cube_mesh(&atlas, "water");
        let (mut renderer, log) = recorder();
        let extent = [16.0; 3];
        renderer
            .set_section([0, 0, -1], [0.0, 0.0, -16.0], extent, &water)
            .unwrap();
        renderer
            .set_section([0, 0, -3], [0.0, 0.0, -48.0], extent, &water)
            .unwrap();
        renderer
            .set_section([0, 0, -2], [0.0, 0.0, -32.0], extent, &water)
            .unwrap();
        renderer
            .draw_frame(&looking_north([8.0, 8.0, 8.0]), [0; 4])
            .unwrap();
        let events = log.events.borrow();
        assert_eq!(
            events.as_slice(),
            [
                "begin",
                "Translucent:2",
                "Translucent:3",
                "Translucent:1",
                "end"
            ]
        );
    }

    #[test]
    fn replacing_and_removing_sections_frees_their_meshes() {
        let atlas = atlas();
        let mesh = cube_mesh(&atlas, "solid");
        let (mut renderer, log) = recorder();
        renderer
            .set_section([0, 0, 0], [0.0; 3], [16.0; 3], &mesh)
            .unwrap();
        renderer
            .set_section([0, 0, 0], [0.0; 3], [16.0; 3], &mesh)
            .unwrap();
        assert_eq!((renderer.section_count(), log.freed.get()), (1, 1));
        renderer
            .set_section([0, 0, 0], [0.0; 3], [16.0; 3], &Mesh::default())
            .unwrap();
        assert_eq!((renderer.section_count(), log.freed.get()), (0, 2));
        renderer
            .set_section([1, 0, 0], [16.0, 0.0, 0.0], [16.0; 3], &mesh)
            .unwrap();
        assert!(renderer.remove_section([1, 0, 0]));
        assert!(!renderer.remove_section([1, 0, 0]));
        assert_eq!(log.freed.get(), 3);
    }

    #[test]
    fn a_failed_upload_leaves_the_old_section_in_place() {
        struct Failing(bool);
        impl Backend for Failing {
            type Error = &'static str;
            type Mesh = ();
            fn upload_atlas(&mut self, _: &Atlas) -> Result<(), &'static str> {
                Ok(())
            }
            fn upload_mesh(&mut self, _: &Mesh) -> Result<(), &'static str> {
                if self.0 { Err("out of memory") } else { Ok(()) }
            }
            fn begin_frame(&mut self, _: [u8; 4]) {}
            fn draw(&mut self, _: &(), _: Layer, _: &Mat4) {}
            fn end_frame(&mut self) -> Result<(), &'static str> {
                Ok(())
            }
        }
        let atlas = atlas();
        let mesh = cube_mesh(&atlas, "solid");
        let mut renderer = Renderer::new(Failing(false));
        renderer
            .set_section([0; 3], [0.0; 3], [16.0; 3], &mesh)
            .unwrap();
        renderer.backend_mut().0 = true;
        assert_eq!(
            renderer.set_section([0; 3], [0.0; 3], [16.0; 3], &mesh),
            Err("out of memory")
        );
        assert_eq!(renderer.section_count(), 1);
    }

    fn software(width: u32) -> Renderer<SoftwareBackend> {
        let mut backend = SoftwareBackend::new(width, width);
        backend.upload_atlas(&atlas()).unwrap();
        Renderer::new(backend)
    }

    #[test]
    fn the_software_backend_shows_what_the_camera_sees_as_it_flies() {
        let atlas = atlas();
        let solid = cube_mesh(&atlas, "solid");
        let mut renderer = software(64);
        renderer
            .set_section([0, 0, 0], [0.0; 3], [1.0; 3], &solid)
            .unwrap();

        let mut camera = Camera::new([0.5, 0.5, 4.0], 1.0);
        camera.yaw = 180.0;
        let sky = [10, 20, 30, 255];
        renderer.draw_frame(&camera, sky).unwrap();
        assert_eq!(
            renderer.backend().frame.pixel(32, 32),
            [160, 40, 40, 255],
            "south face at 0.8"
        );

        let covered = |r: &Renderer<SoftwareBackend>| {
            r.backend()
                .frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| **p != sky)
                .count()
        };
        let near = covered(&renderer);
        assert!(
            near > 100,
            "close up the block fills a good part of the view: {near}"
        );

        camera.fly(
            FlyInput {
                forward: -1.0,
                ..FlyInput::default()
            },
            1.0,
        );
        renderer.draw_frame(&camera, sky).unwrap();
        let far = covered(&renderer);
        assert!(
            (1..30).contains(&far),
            "ten blocks further away it is a few pixels: {far}"
        );
        assert_eq!(renderer.backend().frame.pixel(0, 0), sky);

        camera.look(180.0, 0.0);
        renderer.draw_frame(&camera, sky).unwrap();
        assert_eq!(
            renderer.backend().frame.pixel(32, 32),
            sky,
            "facing away sees only sky"
        );
    }

    #[test]
    fn translucent_sections_blend_over_the_opaque_ones_behind_them() {
        let atlas = atlas();
        let solid = cube_mesh(&atlas, "solid");
        let water = cube_mesh(&atlas, "water");
        let mut renderer = software(64);
        renderer
            .set_section([0, 0, -2], [0.0, 0.0, -2.0], [1.0; 3], &solid)
            .unwrap();
        renderer
            .set_section([0, 0, 0], [0.0; 3], [1.0; 3], &water)
            .unwrap();
        let mut camera = Camera::new([0.5, 0.5, 4.0], 1.0);
        camera.yaw = 180.0;
        renderer.draw_frame(&camera, [0, 0, 0, 255]).unwrap();
        let p = renderer.backend().frame.pixel(32, 32);
        assert!(
            p[0] > 50 && p[2] > 50,
            "red from behind and blue from in front: {p:?}"
        );
    }

    #[test]
    fn drawing_without_an_atlas_is_an_error() {
        let mut backend = SoftwareBackend::new(4, 4);
        backend.begin_frame([0; 4]);
        assert_eq!(backend.end_frame(), Err(NoAtlas));
    }
}
