//! A small CPU rasterizer. The game never uses it: it exists so tests and debugging tools can
//! see what a mesh looks like without a GPU. It follows the same conventions a GPU would:
//! counter-clockwise triangles face the viewer, depth runs from -1 to 1, and texture
//! coordinates are perspective correct.

use crate::atlas::Atlas;
use crate::math::Mat4;
use crate::mesh::{Batch, Mesh, Vertex};
use crate::quad::Layer;

const CUTOUT_THRESHOLD: u8 = 26;

pub struct Framebuffer {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    depth: Vec<f32>,
}

impl Framebuffer {
    pub fn new(width: u32, height: u32, clear: [u8; 4]) -> Self {
        Self {
            width,
            height,
            rgba: clear.repeat(width as usize * height as usize),
            depth: vec![f32::INFINITY; width as usize * height as usize],
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.rgba[at],
            self.rgba[at + 1],
            self.rgba[at + 2],
            self.rgba[at + 3],
        ]
    }
}

#[derive(Clone, Copy)]
struct ClipVertex {
    position: [f32; 4],
    uv: [f32; 2],
    color: [f32; 4],
}

fn lerp(a: &ClipVertex, b: &ClipVertex, t: f32) -> ClipVertex {
    let mix = |x: f32, y: f32| x + (y - x) * t;
    ClipVertex {
        position: std::array::from_fn(|i| mix(a.position[i], b.position[i])),
        uv: std::array::from_fn(|i| mix(a.uv[i], b.uv[i])),
        color: std::array::from_fn(|i| mix(a.color[i], b.color[i])),
    }
}

/// Clips a polygon to the near plane, where clip depth equals minus the clip w.
fn clip_near(polygon: &[ClipVertex]) -> Vec<ClipVertex> {
    let inside = |v: &ClipVertex| v.position[2] + v.position[3];
    let mut out = Vec::with_capacity(polygon.len() + 1);
    for (i, current) in polygon.iter().enumerate() {
        let next = &polygon[(i + 1) % polygon.len()];
        let (dc, dn) = (inside(current), inside(next));
        if dc >= 0.0 {
            out.push(*current);
        }
        if (dc >= 0.0) != (dn >= 0.0) {
            out.push(lerp(current, next, dc / (dc - dn)));
        }
    }
    out
}

struct Screen {
    x: f32,
    y: f32,
    z: f32,
    inverse_w: f32,
    uv_over_w: [f32; 2],
    color_over_w: [f32; 4],
}

fn to_screen(v: &ClipVertex, width: u32, height: u32) -> Screen {
    let w = v.position[3];
    let inverse_w = 1.0 / w;
    Screen {
        x: (v.position[0] * inverse_w * 0.5 + 0.5) * width as f32,
        y: (1.0 - (v.position[1] * inverse_w * 0.5 + 0.5)) * height as f32,
        z: v.position[2] * inverse_w,
        inverse_w,
        uv_over_w: [v.uv[0] * inverse_w, v.uv[1] * inverse_w],
        color_over_w: std::array::from_fn(|i| v.color[i] * inverse_w),
    }
}

struct Pass<'a> {
    atlas: &'a Atlas,
    layer: Layer,
}

fn shade_pixel(
    frame: &mut Framebuffer,
    pass: &Pass,
    x: u32,
    y: u32,
    depth: f32,
    uv: [f32; 2],
    color: [f32; 4],
) {
    let at = y as usize * frame.width as usize + x as usize;
    if depth >= frame.depth[at] {
        return;
    }
    let tx = ((uv[0] * pass.atlas.width as f32).floor() as i64)
        .clamp(0, i64::from(pass.atlas.width) - 1) as usize;
    let ty = ((uv[1] * pass.atlas.height as f32).floor() as i64)
        .clamp(0, i64::from(pass.atlas.height) - 1) as usize;
    let from = (ty * pass.atlas.width as usize + tx) * 4;
    let texel = &pass.atlas.rgba[from..from + 4];
    let alpha = if pass.layer == Layer::Solid {
        255
    } else {
        texel[3]
    };
    if pass.layer == Layer::Cutout && alpha < CUTOUT_THRESHOLD {
        return;
    }
    let lit: [f32; 3] = std::array::from_fn(|i| f32::from(texel[i]) * color[i]);
    let target = &mut frame.rgba[at * 4..at * 4 + 4];
    if pass.layer == Layer::Translucent {
        let a = f32::from(alpha) / 255.0;
        for i in 0..3 {
            target[i] = (lit[i] * a + f32::from(target[i]) * (1.0 - a)).round() as u8;
        }
        return;
    }
    for i in 0..3 {
        target[i] = lit[i].round().clamp(0.0, 255.0) as u8;
    }
    target[3] = 255;
    frame.depth[at] = depth;
}

fn draw_triangle(frame: &mut Framebuffer, pass: &Pass, a: &Screen, b: &Screen, c: &Screen) {
    let area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
    if area >= 0.0 {
        return;
    }
    let min_x = a.x.min(b.x).min(c.x).floor().max(0.0) as u32;
    let max_x = (a.x.max(b.x).max(c.x).ceil().min(frame.width as f32)) as u32;
    let min_y = a.y.min(b.y).min(c.y).floor().max(0.0) as u32;
    let max_y = (a.y.max(b.y).max(c.y).ceil().min(frame.height as f32)) as u32;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((b.x - p.0) * (c.y - p.1) - (c.x - p.0) * (b.y - p.1)) / area;
            let w1 = ((c.x - p.0) * (a.y - p.1) - (a.x - p.0) * (c.y - p.1)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let inverse_w = w0 * a.inverse_w + w1 * b.inverse_w + w2 * c.inverse_w;
            let mix2 = |f: fn(&Screen) -> [f32; 2]| -> [f32; 2] {
                std::array::from_fn(|i| (w0 * f(a)[i] + w1 * f(b)[i] + w2 * f(c)[i]) / inverse_w)
            };
            let uv = mix2(|s| s.uv_over_w);
            let color: [f32; 4] = std::array::from_fn(|i| {
                (w0 * a.color_over_w[i] + w1 * b.color_over_w[i] + w2 * c.color_over_w[i])
                    / inverse_w
                    / 255.0
            });
            let depth = w0 * a.z + w1 * b.z + w2 * c.z;
            shade_pixel(frame, pass, x, y, depth, uv, color);
        }
    }
}

fn draw_batch(
    frame: &mut Framebuffer,
    pass: &Pass,
    batch: &Batch,
    transform: &Mat4,
    offset: [f32; 3],
) {
    let corner = |v: &Vertex| ClipVertex {
        position: transform.transform([
            v.pos[0] + offset[0],
            v.pos[1] + offset[1],
            v.pos[2] + offset[2],
            1.0,
        ]),
        uv: v.uv,
        color: [
            f32::from(v.color[0]),
            f32::from(v.color[1]),
            f32::from(v.color[2]),
            255.0,
        ],
    };
    for triangle in batch.indices.as_chunks::<3>().0 {
        let polygon: Vec<ClipVertex> = triangle
            .iter()
            .map(|&i| corner(&batch.vertices[usize::from(i)]))
            .collect();
        let clipped = clip_near(&polygon);
        if clipped.len() < 3 {
            continue;
        }
        let screen: Vec<Screen> = clipped
            .iter()
            .map(|v| to_screen(v, frame.width, frame.height))
            .collect();
        for i in 1..screen.len() - 1 {
            draw_triangle(frame, pass, &screen[0], &screen[i], &screen[i + 1]);
        }
    }
}

/// Draws a mesh: solid first, then cutout, then translucent so blending sees what is behind.
/// `offset` moves the mesh's local coordinates into the world.
pub fn render(
    frame: &mut Framebuffer,
    mesh: &Mesh,
    atlas: &Atlas,
    view_projection: &Mat4,
    offset: [f32; 3],
) {
    for layer in Layer::ALL {
        render_layer(frame, mesh, layer, atlas, view_projection, offset);
    }
}

/// Draws one layer of a mesh.
pub fn render_layer(
    frame: &mut Framebuffer,
    mesh: &Mesh,
    layer: Layer,
    atlas: &Atlas,
    view_projection: &Mat4,
    offset: [f32; 3],
) {
    let pass = Pass { atlas, layer };
    for batch in mesh.layer(layer) {
        draw_batch(frame, &pass, batch, view_projection, offset);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use psv_pack::texture::{FLAG_HAS_TRANSLUCENT, FLAG_HAS_TRANSPARENT};

    use super::*;
    use crate::atlas::AtlasEntry;
    use crate::camera::Camera;
    use crate::geometry::Rotation;
    use crate::mesh::{Appearance, AppearanceProvider, BlockView, NoTint, Tinter, mesh_region};
    use crate::model::{ModelSource, resolve};
    use crate::quad::bake;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const YELLOW: [u8; 4] = [255, 255, 0, 255];

    struct Models(HashMap<String, String>);

    impl ModelSource for Models {
        fn load(&mut self, location: &str) -> Option<String> {
            self.0.get(location).cloned()
        }
    }

    struct Palette(HashMap<u32, Appearance>);

    impl AppearanceProvider for Palette {
        fn appearance(&self, state: u32) -> Option<&Appearance> {
            self.0.get(&state)
        }
    }

    struct Grid(HashMap<[i32; 3], u32>);

    impl BlockView for Grid {
        fn state(&self, x: i32, y: i32, z: i32) -> u32 {
            self.0.get(&[x, y, z]).copied().unwrap_or(0)
        }
    }

    fn entry(
        name: &str,
        w: u32,
        h: u32,
        flags: u8,
        pixel: impl Fn(u32, u32) -> [u8; 4],
    ) -> AtlasEntry {
        AtlasEntry {
            name: name.into(),
            width: w,
            height: h,
            rgba: (0..w * h).flat_map(|i| pixel(i % w, i / w)).collect(),
            flags,
        }
    }

    fn quadrants(name: &str) -> AtlasEntry {
        entry(name, 16, 16, 0, |x, y| match (x < 8, y < 8) {
            (true, true) => RED,
            (false, true) => GREEN,
            (true, false) => BLUE,
            (false, false) => YELLOW,
        })
    }

    fn atlas(entries: Vec<AtlasEntry>) -> Atlas {
        Atlas::build(entries, 1, 256).unwrap()
    }

    fn face_json(textures: [&str; 6]) -> String {
        let names = ["down", "up", "north", "south", "west", "east"];
        let faces: Vec<String> = names
            .iter()
            .zip(textures)
            .map(|(n, t)| format!(r#""{n}":{{"texture":"{t}","cullface":"{n}","tintindex":-1}}"#))
            .collect();
        format!(
            r#"{{"elements":[{{"from":[0,0,0],"to":[16,16,16],"faces":{{{}}}}}]}}"#,
            faces.join(",")
        )
    }

    fn appearance(json: &str, atlas: &Atlas, rotation: Rotation) -> Appearance {
        let mut source = Models([("minecraft:m".to_owned(), json.to_owned())].into());
        let model = resolve("m", &mut source).unwrap();
        let (quads, report) = bake(&model, rotation, atlas);
        assert!(
            report.missing_sprites.is_empty() && report.unresolved_variables.is_empty(),
            "{report:?}"
        );
        Appearance::new(quads)
    }

    fn look(
        camera: Camera,
        grid: &Grid,
        palette: &Palette,
        atlas: &Atlas,
        tint: &dyn Tinter,
    ) -> Framebuffer {
        let mesh = mesh_region(grid, palette, tint, [-8, -8, -8], [16, 16, 16]);
        let mut frame = Framebuffer::new(64, 64, [10, 10, 10, 255]);
        render(
            &mut frame,
            &mesh,
            atlas,
            &camera.view_projection(),
            [-8.0, -8.0, -8.0],
        );
        frame
    }

    fn camera(position: [f32; 3], yaw: f32, pitch: f32) -> Camera {
        let mut c = Camera::new(position, 1.0);
        c.yaw = yaw;
        c.pitch = pitch;
        c
    }

    fn single(appearance: Appearance) -> (Grid, Palette) {
        (
            Grid([([0, 0, 0], 1)].into()),
            Palette([(1, appearance)].into()),
        )
    }

    fn lit(color: [u8; 4], light: f32) -> [u8; 4] {
        [
            (f32::from(color[0]) * light).round() as u8,
            (f32::from(color[1]) * light).round() as u8,
            (f32::from(color[2]) * light).round() as u8,
            255,
        ]
    }

    fn six_colors() -> Atlas {
        let color = |n: &str, c: [u8; 4]| entry(n, 16, 16, 0, move |_, _| c);
        atlas(vec![
            color("minecraft:down", [90, 90, 90, 255]),
            color("minecraft:up", [200, 50, 50, 255]),
            color("minecraft:north", [50, 200, 50, 255]),
            color("minecraft:south", [50, 50, 200, 255]),
            color("minecraft:west", [200, 200, 50, 255]),
            color("minecraft:east", [50, 200, 200, 255]),
        ])
    }

    #[test]
    fn each_side_of_a_block_is_seen_from_outside_with_its_own_shade() {
        let atlas = six_colors();
        let json = face_json(["down", "up", "north", "south", "west", "east"]);
        let (grid, palette) = single(appearance(&json, &atlas, Rotation::NONE));
        let views = [
            ("up", [0.5, 3.0, 0.5], 0.0, 89.9, [200, 50, 50, 255], 1.0),
            ("down", [0.5, -2.0, 0.5], 0.0, -89.9, [90, 90, 90, 255], 0.5),
            (
                "south",
                [0.5, 0.5, 3.0],
                180.0,
                0.0,
                [50, 50, 200, 255],
                0.8,
            ),
            ("north", [0.5, 0.5, -2.0], 0.0, 0.0, [50, 200, 50, 255], 0.8),
            (
                "west",
                [-2.0, 0.5, 0.5],
                270.0,
                0.0,
                [200, 200, 50, 255],
                0.6,
            ),
            ("east", [3.0, 0.5, 0.5], 90.0, 0.0, [50, 200, 200, 255], 0.6),
        ];
        for (name, position, yaw, pitch, color, light) in views {
            let frame = look(
                camera(position, yaw, pitch),
                &grid,
                &palette,
                &atlas,
                &NoTint,
            );
            assert_eq!(frame.pixel(32, 32), lit(color, light), "{name} face");
        }
    }

    fn upright(frame: &Framebuffer) -> [[u8; 4]; 4] {
        [
            frame.pixel(24, 24),
            frame.pixel(40, 24),
            frame.pixel(24, 40),
            frame.pixel(40, 40),
        ]
    }

    #[test]
    fn textures_appear_upright_and_unmirrored_on_every_side() {
        let atlas = atlas(vec![quadrants("minecraft:q")]);
        let json = face_json(["q"; 6]);
        let (grid, palette) = single(appearance(&json, &atlas, Rotation::NONE));
        let expected = |light| {
            [
                lit(RED, light),
                lit(GREEN, light),
                lit(BLUE, light),
                lit(YELLOW, light),
            ]
        };
        for (name, position, yaw, light) in [
            ("south", [0.5, 0.5, 2.2], 180.0, 0.8),
            ("north", [0.5, 0.5, -1.2], 0.0, 0.8),
            ("east", [2.2, 0.5, 0.5], 90.0, 0.6),
            ("west", [-1.2, 0.5, 0.5], 270.0, 0.6),
        ] {
            let frame = look(camera(position, yaw, 0.0), &grid, &palette, &atlas, &NoTint);
            assert_eq!(upright(&frame), expected(light), "{name}");
        }
        let top = look(
            camera([0.5, 2.2, 0.5], 180.0, 89.9),
            &grid,
            &palette,
            &atlas,
            &NoTint,
        );
        assert_eq!(
            upright(&top),
            expected(1.0),
            "top, north at the top of the screen"
        );
    }

    #[test]
    fn texture_rotation_turns_the_picture_a_quarter_clockwise() {
        let atlas = atlas(vec![quadrants("minecraft:q")]);
        let json = r#"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{
            "south":{"texture":"q","rotation":90,"cullface":"south"}}}]}"#;
        let (grid, palette) = single(appearance(json, &atlas, Rotation::NONE));
        let frame = look(
            camera([0.5, 0.5, 2.2], 180.0, 0.0),
            &grid,
            &palette,
            &atlas,
            &NoTint,
        );
        assert_eq!(
            upright(&frame),
            [
                lit(BLUE, 0.8),
                lit(RED, 0.8),
                lit(YELLOW, 0.8),
                lit(GREEN, 0.8)
            ]
        );
    }

    #[test]
    fn a_rotated_block_shows_its_north_face_to_the_east() {
        let atlas = six_colors();
        let json = face_json(["down", "up", "north", "south", "west", "east"]);
        let (grid, palette) = single(appearance(
            &json,
            &atlas,
            Rotation::from_degrees(0, 90).unwrap(),
        ));
        let frame = look(
            camera([3.0, 0.5, 0.5], 90.0, 0.0),
            &grid,
            &palette,
            &atlas,
            &NoTint,
        );
        assert_eq!(frame.pixel(32, 32), lit([50, 200, 50, 255], 0.6));
    }

    #[test]
    fn hidden_faces_are_not_meshed() {
        let atlas = six_colors();
        let json = face_json(["down", "up", "north", "south", "west", "east"]);
        let cube = appearance(&json, &atlas, Rotation::NONE);
        let palette = Palette([(1, cube)].into());
        let one = Grid([([0, 0, 0], 1)].into());
        let two = Grid([([0, 0, 0], 1), ([1, 0, 0], 1)].into());
        let count =
            |g: &Grid| mesh_region(g, &palette, &NoTint, [-2, -2, -2], [6, 6, 6]).quad_count();
        assert_eq!((count(&one), count(&two)), (6, 10));
    }

    #[test]
    fn solid_blocks_hide_glass_faces_but_glass_does_not_hide_solid_faces() {
        let color = |n: &str, f| {
            entry(n, 4, 4, f, |x, _| {
                if x < 2 { [9, 9, 9, 255] } else { [0, 0, 0, 0] }
            })
        };
        let atlas = atlas(vec![
            color("minecraft:solid", 0),
            color("minecraft:glass", FLAG_HAS_TRANSPARENT),
        ]);
        let solid = appearance(&face_json(["solid"; 6]), &atlas, Rotation::NONE);
        let glass = appearance(&face_json(["glass"; 6]), &atlas, Rotation::NONE);
        let palette = Palette([(1, solid), (2, glass)].into());
        let grid = Grid([([0, 0, 0], 1), ([1, 0, 0], 2)].into());
        let mesh = mesh_region(&grid, &palette, &NoTint, [-2, -2, -2], [6, 6, 6]);
        assert_eq!(
            mesh.layer(Layer::Solid)[0].indices.len() / 6,
            6,
            "the solid keeps all six faces"
        );
        assert_eq!(
            mesh.layer(Layer::Cutout)[0].indices.len() / 6,
            5,
            "the glass loses its hidden face"
        );
    }

    #[test]
    fn identical_translucent_blocks_do_not_draw_the_faces_between_them() {
        let atlas = atlas(vec![entry(
            "minecraft:ice",
            4,
            4,
            FLAG_HAS_TRANSLUCENT,
            |_, _| [100, 150, 250, 128],
        )]);
        let ice = appearance(&face_json(["ice"; 6]), &atlas, Rotation::NONE);
        let palette = Palette([(1, ice)].into());
        let grid = Grid([([0, 0, 0], 1), ([1, 0, 0], 1)].into());
        let mesh = mesh_region(&grid, &palette, &NoTint, [-2, -2, -2], [6, 6, 6]);
        assert_eq!(mesh.quad_count(), 10);
    }

    #[test]
    fn cutout_pixels_are_discarded_and_translucent_ones_blend() {
        let atlas = atlas(vec![
            entry("minecraft:leaf", 16, 16, FLAG_HAS_TRANSPARENT, |x, _| {
                if x < 8 {
                    [0, 200, 0, 255]
                } else {
                    [0, 0, 0, 0]
                }
            }),
            entry("minecraft:wall", 16, 16, 0, |_, _| [100, 100, 100, 255]),
            entry("minecraft:pane", 16, 16, FLAG_HAS_TRANSLUCENT, |_, _| {
                [200, 0, 0, 128]
            }),
        ]);
        let mk = |t| appearance(&face_json([t; 6]), &atlas, Rotation::NONE);
        let palette = Palette([(1, mk("leaf")), (2, mk("wall")), (3, mk("pane"))].into());

        let behind = Grid([([0, 0, 0], 1), ([0, 0, -2], 2)].into());
        let frame = look(
            camera([0.5, 0.5, 3.0], 180.0, 0.0),
            &behind,
            &palette,
            &atlas,
            &NoTint,
        );
        assert_eq!(
            frame.pixel(26, 32),
            lit([0, 200, 0, 255], 0.8),
            "opaque half of the leaf"
        );
        assert_eq!(
            frame.pixel(35, 32),
            lit([100, 100, 100, 255], 0.8),
            "wall through the cut out half"
        );

        let glass = Grid([([0, 0, 0], 3), ([0, 0, -2], 2)].into());
        let frame = look(
            camera([0.5, 0.5, 3.0], 180.0, 0.0),
            &glass,
            &palette,
            &atlas,
            &NoTint,
        );
        let blended = frame.pixel(32, 32);
        assert!(
            blended[0] > 100 && blended[1] < 100,
            "red over grey: {blended:?}"
        );
    }

    #[test]
    fn tint_and_shade_multiply() {
        struct Green;
        impl Tinter for Green {
            fn tint(&self, _: i32, _: i32, _: i32, _: i32) -> [f32; 3] {
                [0.5, 1.0, 0.25]
            }
        }
        let atlas = atlas(vec![entry("minecraft:grass", 4, 4, 0, |_, _| {
            [200, 200, 200, 255]
        })]);
        let json = r#"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{
            "up":{"texture":"grass","tintindex":0,"cullface":"up"}}}]}"#;
        let (grid, palette) = single(appearance(json, &atlas, Rotation::NONE));
        let frame = look(
            camera([0.5, 3.0, 0.5], 0.0, 89.9),
            &grid,
            &palette,
            &atlas,
            &Green,
        );
        assert_eq!(frame.pixel(32, 32), [100, 200, 50, 255]);
    }

    #[test]
    fn large_meshes_split_into_batches_with_valid_16_bit_indices() {
        let atlas = atlas(vec![entry("minecraft:s", 4, 4, 0, |_, _| [1, 2, 3, 255])]);
        let (_, palette) = single(appearance(&face_json(["s"; 6]), &atlas, Rotation::NONE));
        let mut cells = HashMap::new();
        for x in 0..24_i32 {
            for y in 0..24_i32 {
                for z in 0..24_i32 {
                    if (x + y + z) % 2 == 0 {
                        cells.insert([x, y, z], 1);
                    }
                }
            }
        }
        let mesh = mesh_region(&Grid(cells), &palette, &NoTint, [0; 3], [24; 3]);
        let batches = mesh.layer(Layer::Solid);
        assert_eq!(mesh.quad_count(), 6912 * 6);
        assert!(batches.len() >= 3);
        for batch in batches {
            assert!(batch.vertices.len() <= crate::mesh::MAX_BATCH_VERTICES);
            assert!(
                batch
                    .indices
                    .iter()
                    .all(|&i| usize::from(i) < batch.vertices.len())
            );
        }
    }

    #[test]
    fn nothing_is_drawn_for_empty_or_unknown_blocks() {
        let grid = Grid(HashMap::new());
        let palette = Palette(HashMap::new());
        assert!(mesh_region(&grid, &palette, &NoTint, [0; 3], [8; 3]).is_empty());
        let grid = Grid([([0, 0, 0], 99)].into());
        assert!(mesh_region(&grid, &palette, &NoTint, [0; 3], [8; 3]).is_empty());
    }

    #[test]
    fn a_floor_running_behind_the_camera_is_clipped_not_smeared() {
        let atlas = six_colors();
        let json = face_json(["down", "up", "north", "south", "west", "east"]);
        let cube = appearance(&json, &atlas, Rotation::NONE);
        let palette = Palette([(1, cube)].into());
        let grid = Grid((0..8).map(|z| ([0, 0, z], 1)).collect());
        let frame = look(
            camera([0.5, 1.6, 5.5], 180.0, 30.0),
            &grid,
            &palette,
            &atlas,
            &NoTint,
        );
        assert_eq!(
            frame.pixel(32, 60),
            lit([200, 50, 50, 255], 1.0),
            "the floor ahead and below"
        );
        assert_eq!(frame.pixel(0, 0), [10, 10, 10, 255], "sky stays empty");
        assert_eq!(frame.pixel(63, 0), [10, 10, 10, 255], "sky stays empty");
    }

    #[test]
    fn a_face_closer_than_the_near_plane_is_dropped_and_reappears_when_it_is_not() {
        let atlas = six_colors();
        let json = face_json(["down", "up", "north", "south", "west", "east"]);
        let (grid, palette) = single(appearance(&json, &atlas, Rotation::NONE));
        let too_close = look(
            camera([0.5, 1.02, 0.5], 0.0, 89.9),
            &grid,
            &palette,
            &atlas,
            &NoTint,
        );
        assert_eq!(too_close.pixel(32, 32), [10, 10, 10, 255]);
        let clear = look(
            camera([0.5, 1.5, 0.5], 0.0, 89.9),
            &grid,
            &palette,
            &atlas,
            &NoTint,
        );
        assert_eq!(clear.pixel(32, 32), lit([200, 50, 50, 255], 1.0));
    }

    #[test]
    fn back_faces_are_not_drawn_from_inside_a_block() {
        let atlas = six_colors();
        let json = face_json(["down", "up", "north", "south", "west", "east"]);
        let (grid, palette) = single(appearance(&json, &atlas, Rotation::NONE));
        let inside = look(
            camera([0.5, 0.5, 0.5], 180.0, 0.0),
            &grid,
            &palette,
            &atlas,
            &NoTint,
        );
        assert_eq!(inside.pixel(32, 32), [10, 10, 10, 255]);
    }
}
