//! The demo world: terrain from Horizon's own Perlin noise, turned into chunk meshes.
//!
//! It draws from a real `data.pak` when there is one, and from generated placeholder art
//! otherwise, so the graphics can be tried without any game files.

use std::io::{Cursor, Read, Seek};

use horizon_core::noise::PerlinNoise;
use horizon_core::random::XoroshiroRandom;
use psv_pack::{CodecChoice, PackError, PackReader, PackWriter, texture};
use psv_render::atlas::{Atlas, AtlasError, entries_from_pack};
use psv_render::blockstate::Properties;
use psv_render::mesh::{Appearance, AppearanceProvider, BlockView, Mesh, Tinter, mesh_region};
use psv_render::registry::{RegistryError, build_appearance};
use thiserror::Error;

pub const AIR: u32 = 0;
pub const STONE: u32 = 1;
pub const DIRT: u32 = 2;
pub const GRASS: u32 = 3;
pub const SAND: u32 = 4;

const COLUMNS: i32 = 64;
const SECTION: i32 = 16;
const SECTIONS_Y: i32 = 3;
const SAND_LEVEL: i32 = 22;
const TERRAIN_SEED: i64 = 0x004D_434A;
const GRASS_COLOR: [f32; 3] = [0.569, 0.741, 0.349];

const BLOCK_TEXTURES: &[&str] = &[
    "minecraft:block/stone",
    "minecraft:block/dirt",
    "minecraft:block/sand",
    "minecraft:block/grass_block_top",
    "minecraft:block/grass_block_side",
    "minecraft:block/grass_block_side_overlay",
    "minecraft:block/grass_block_snow",
];

#[derive(Debug, Error)]
pub enum SceneError {
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Atlas(#[from] AtlasError),
    #[error(transparent)]
    Pack(#[from] PackError),
    #[error("{block} refers to textures that are not in the pack: {missing:?}")]
    MissingTextures { block: String, missing: Vec<String> },
}

pub struct Terrain {
    heights: Vec<i32>,
}

impl Terrain {
    pub fn generate() -> Self {
        let noise = PerlinNoise::new(&mut XoroshiroRandom::new(TERRAIN_SEED));
        let mut heights = Vec::with_capacity((COLUMNS * COLUMNS) as usize);
        for z in 0..COLUMNS {
            for x in 0..COLUMNS {
                let (fx, fz) = (f64::from(x), f64::from(z));
                let n = noise.get3(fx * 0.03, 0.0, fz * 0.03)
                    + 0.5 * noise.get3(fx * 0.07, 11.0, fz * 0.07)
                    + 0.25 * noise.get3(fx * 0.15, 23.0, fz * 0.15);
                heights.push((24.0 + n * 7.0).round().clamp(4.0, 44.0) as i32);
            }
        }
        Self { heights }
    }

    pub fn height(&self, x: i32, z: i32) -> Option<i32> {
        ((0..COLUMNS).contains(&x) && (0..COLUMNS).contains(&z))
            .then(|| self.heights[(z * COLUMNS + x) as usize])
    }
}

impl BlockView for Terrain {
    fn state(&self, x: i32, y: i32, z: i32) -> u32 {
        let Some(top) = self.height(x, z) else {
            return AIR;
        };
        match y {
            y if y < 0 || y > top => AIR,
            y if y == top => {
                if top <= SAND_LEVEL {
                    SAND
                } else {
                    GRASS
                }
            }
            y if y >= top - 3 => DIRT,
            _ => STONE,
        }
    }
}

struct Palette {
    stone: Appearance,
    dirt: Appearance,
    grass: Appearance,
    sand: Appearance,
}

impl AppearanceProvider for Palette {
    fn appearance(&self, state: u32) -> Option<&Appearance> {
        match state {
            STONE => Some(&self.stone),
            DIRT => Some(&self.dirt),
            GRASS => Some(&self.grass),
            SAND => Some(&self.sand),
            _ => None,
        }
    }
}

struct PlainsTint;

impl Tinter for PlainsTint {
    fn tint(&self, _index: i32, _x: i32, _y: i32, _z: i32) -> [f32; 3] {
        GRASS_COLOR
    }
}

pub struct SectionMesh {
    pub key: [i32; 3],
    pub origin: [f32; 3],
    pub mesh: Mesh,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SceneStats {
    pub textures: usize,
    pub sections: usize,
    pub quads: usize,
    pub vertices: usize,
}

pub struct Scene {
    pub atlas: Atlas,
    pub sections: Vec<SectionMesh>,
    pub spawn: [f32; 3],
    pub stats: SceneStats,
}

fn block<R: Read + Seek>(
    pack: &mut PackReader<R>,
    atlas: &Atlas,
    name: &str,
    properties: &Properties,
) -> Result<Appearance, SceneError> {
    let built = build_appearance(pack, atlas, name, properties, 0)?;
    let mut missing = built.report.missing_sprites;
    missing.extend(built.report.unresolved_variables);
    if !missing.is_empty() {
        return Err(SceneError::MissingTextures {
            block: name.to_owned(),
            missing,
        });
    }
    Ok(built.appearance)
}

/// Builds the world from a data pack that holds the blocks' blockstates, models and textures.
pub fn build<R: Read + Seek>(pack: &mut PackReader<R>) -> Result<Scene, SceneError> {
    let entries = entries_from_pack(pack, |name| BLOCK_TEXTURES.contains(&name))?;
    let textures = entries.len();
    let atlas = Atlas::build(entries, 1, 1024)?;

    let none = Properties::new();
    let snowy_no: Properties = [("snowy".to_owned(), "false".to_owned())].into();
    let palette = Palette {
        stone: block(pack, &atlas, "stone", &none)?,
        dirt: block(pack, &atlas, "dirt", &none)?,
        grass: block(pack, &atlas, "grass_block", &snowy_no)?,
        sand: block(pack, &atlas, "sand", &none)?,
    };

    let terrain = Terrain::generate();
    let mut sections = Vec::new();
    let mut stats = SceneStats {
        textures,
        ..SceneStats::default()
    };
    for sx in 0..COLUMNS / SECTION {
        for sz in 0..COLUMNS / SECTION {
            for sy in 0..SECTIONS_Y {
                let min = [sx * SECTION, sy * SECTION, sz * SECTION];
                let mesh = mesh_region(&terrain, &palette, &PlainsTint, min, [SECTION; 3]);
                if mesh.is_empty() {
                    continue;
                }
                stats.quads += mesh.quad_count();
                stats.vertices += mesh.vertex_count();
                sections.push(SectionMesh {
                    key: [sx, sy, sz],
                    origin: [min[0] as f32, min[1] as f32, min[2] as f32],
                    mesh,
                });
            }
        }
    }
    stats.sections = sections.len();

    let centre = COLUMNS / 2;
    let ground = terrain.height(centre, centre).unwrap_or(24);
    Ok(Scene {
        atlas,
        sections,
        spawn: [
            centre as f32 + 0.5,
            ground as f32 + 6.0,
            centre as f32 + 0.5,
        ],
        stats,
    })
}

fn placeholder_pixels(seed: u32, base: [u8; 3], spread: u8) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761) | 1;
    (0..16 * 16)
        .flat_map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let jitter = (state >> 9) as u8 % (spread + 1);
            [
                base[0].saturating_add(jitter),
                base[1].saturating_add(jitter),
                base[2].saturating_add(jitter),
                255,
            ]
        })
        .collect()
}

fn with_strip(mut pixels: Vec<u8>, rows: usize, color: [u8; 3], alpha: u8) -> Vec<u8> {
    for y in 0..rows {
        for x in 0..16 {
            let at = (y * 16 + x) * 4;
            pixels[at..at + 4].copy_from_slice(&[color[0], color[1], color[2], alpha]);
        }
    }
    pixels
}

const CUBE: &str = r##"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{
    "down":{"texture":"#down","cullface":"down"},"up":{"texture":"#up","cullface":"up","tintindex":0},
    "north":{"texture":"#side","cullface":"north"},"south":{"texture":"#side","cullface":"south"},
    "west":{"texture":"#side","cullface":"west"},"east":{"texture":"#side","cullface":"east"}}}]}"##;

/// A data pack of generated art: enough blocks and textures to draw the demo world, with no
/// game files involved.
pub fn placeholder_pack() -> Result<PackReader<Cursor<Vec<u8>>>, PackError> {
    let mut writer = PackWriter::new(Cursor::new(Vec::new()))?;
    let mut add = |path: &str, bytes: &[u8]| writer.add(path, bytes, CodecChoice::Auto);

    let dirt = placeholder_pixels(2, [110, 80, 50], 20);
    let textures: [(&str, Vec<u8>); 6] = [
        ("stone", placeholder_pixels(1, [120, 120, 124], 30)),
        ("dirt", dirt.clone()),
        ("sand", placeholder_pixels(3, [214, 200, 150], 18)),
        (
            "grass_block_top",
            placeholder_pixels(4, [150, 170, 140], 24),
        ),
        (
            "grass_block_side",
            with_strip(dirt, 3, [150, 170, 140], 255),
        ),
        (
            "grass_block_side_overlay",
            with_strip(vec![0; 16 * 16 * 4], 0, [0; 3], 0),
        ),
    ];
    for (name, pixels) in &textures {
        let blob =
            texture::encode(16, 16, pixels).map_err(|_| PackError::Corrupt("generated texture"))?;
        add(
            &format!("assets/minecraft/textures/block/{name}.tex"),
            &blob,
        )?;
    }

    add("assets/minecraft/models/block/cube.json", CUBE.as_bytes())?;
    for (name, up, down, side) in [
        ("stone", "stone", "stone", "stone"),
        ("dirt", "dirt", "dirt", "dirt"),
        ("sand", "sand", "sand", "sand"),
        ("grass_block", "grass_block_top", "dirt", "grass_block_side"),
    ] {
        let model = format!(
            r#"{{"parent":"minecraft:block/cube","textures":{{"up":"block/{up}","down":"block/{down}","side":"block/{side}"}}}}"#
        );
        add(
            &format!("assets/minecraft/models/block/{name}.json"),
            model.as_bytes(),
        )?;
        let key = if name == "grass_block" {
            "snowy=false"
        } else {
            ""
        };
        let state = format!(r#"{{"variants":{{"{key}":{{"model":"minecraft:block/{name}"}}}}}}"#);
        add(
            &format!("assets/minecraft/blockstates/{name}.json"),
            state.as_bytes(),
        )?;
    }
    let (bytes, _) = writer.finish()?;
    PackReader::open(Cursor::new(bytes.into_inner()))
}

#[cfg(test)]
mod tests {
    use psv_render::camera::Camera;
    use psv_render::gpu::{Renderer, SoftwareBackend};

    use super::*;

    #[test]
    fn terrain_is_deterministic_and_stays_in_range() {
        let (a, b) = (Terrain::generate(), Terrain::generate());
        assert_eq!(a.heights, b.heights);
        assert!(a.heights.iter().all(|h| (4..=44).contains(h)));
        let (low, high) = (
            a.heights.iter().min().unwrap(),
            a.heights.iter().max().unwrap(),
        );
        assert!(high - low >= 6, "terrain is flat: {low} to {high}");
        assert_eq!(a.height(-1, 0), None);
        assert_eq!(a.height(0, COLUMNS), None);
    }

    #[test]
    fn columns_have_a_surface_a_dirt_layer_and_stone_below() {
        let t = Terrain::generate();
        let top = t.height(10, 10).unwrap();
        let surface = t.state(10, top, 10);
        assert!(surface == GRASS || surface == SAND);
        assert_eq!(t.state(10, top + 1, 10), AIR);
        assert_eq!(t.state(10, top - 1, 10), DIRT);
        assert_eq!(t.state(10, top - 3, 10), DIRT);
        assert_eq!(t.state(10, top - 4, 10), STONE);
        assert_eq!(t.state(10, -1, 10), AIR);
        assert_eq!(t.state(-5, top, 10), AIR, "outside the world is air");
    }

    #[test]
    fn the_placeholder_world_builds_without_missing_textures() {
        let mut pack = placeholder_pack().unwrap();
        let scene = build(&mut pack).unwrap();
        assert_eq!(scene.stats.textures, 6);
        assert!(
            scene.stats.sections > 4 && scene.stats.quads > 1000,
            "{:?}",
            scene.stats
        );
        assert_eq!(scene.sections.len(), scene.stats.sections);
        assert!(scene.stats.vertices == scene.stats.quads * 4);
        assert!(scene.spawn[1] > 20.0);
        for section in &scene.sections {
            assert!(!section.mesh.is_empty());
        }
    }

    #[test]
    fn building_twice_gives_identical_meshes() {
        let one = build(&mut placeholder_pack().unwrap()).unwrap();
        let two = build(&mut placeholder_pack().unwrap()).unwrap();
        assert_eq!(one.stats, two.stats);
        assert!(
            one.sections
                .iter()
                .zip(&two.sections)
                .all(|(a, b)| a.mesh == b.mesh)
        );
    }

    #[test]
    fn the_camera_at_the_spawn_point_sees_terrain() {
        let scene = build(&mut placeholder_pack().unwrap()).unwrap();
        let mut backend = SoftwareBackend::new(96, 54);
        psv_render::gpu::Backend::upload_atlas(&mut backend, &scene.atlas).unwrap();
        let mut renderer = Renderer::new(backend);
        for s in &scene.sections {
            renderer
                .set_section(s.key, s.origin, [16.0; 3], &s.mesh)
                .unwrap();
        }
        let mut camera = Camera::new(scene.spawn, 96.0 / 54.0);
        camera.pitch = 25.0;
        let sky = [120, 170, 255, 255];
        let stats = renderer.draw_frame(&camera, sky).unwrap();
        assert!(stats.sections_drawn > 0 && stats.draw_calls >= stats.sections_drawn);
        let covered = renderer
            .backend()
            .frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| **p != sky)
            .count();
        let total = 96 * 54;
        assert!(
            covered * 100 / total > 30,
            "only {covered} of {total} pixels are terrain"
        );
        assert_eq!(
            renderer.backend().frame.pixel(0, 0),
            sky,
            "the sky is still sky at the top corner"
        );
    }

    #[test]
    fn grass_tops_are_tinted_green_and_sand_is_not() {
        let scene = build(&mut placeholder_pack().unwrap()).unwrap();
        let mut found_green = false;
        for section in &scene.sections {
            for batch in section.mesh.layer(psv_render::quad::Layer::Solid) {
                for v in &batch.vertices {
                    let [r, g, b, _] = v.color;
                    found_green |= g > r && g > b;
                }
            }
        }
        assert!(found_green, "no vertex carries the grass tint");
    }

    #[test]
    fn a_pack_without_the_blocks_is_an_error_not_a_panic() {
        let mut writer = PackWriter::new(Cursor::new(Vec::new())).unwrap();
        writer
            .add("assets/minecraft/x.json", b"{}", CodecChoice::Store)
            .unwrap();
        let mut pack =
            PackReader::open(Cursor::new(writer.finish().unwrap().0.into_inner())).unwrap();
        assert!(build(&mut pack).is_err());
    }

    #[test]
    fn a_block_with_a_missing_texture_is_an_error_naming_it() {
        let mut writer = PackWriter::new(Cursor::new(Vec::new())).unwrap();
        for (name, pixels) in [
            ("stone", [9; 4].repeat(256)),
            ("dirt", [8; 4].repeat(256)),
            ("sand", [7; 4].repeat(256)),
        ] {
            let blob = texture::encode(16, 16, &pixels).unwrap();
            writer
                .add(
                    &format!("assets/minecraft/textures/block/{name}.tex"),
                    &blob,
                    CodecChoice::Auto,
                )
                .unwrap();
        }
        writer
            .add(
                "assets/minecraft/models/block/cube.json",
                CUBE.as_bytes(),
                CodecChoice::Auto,
            )
            .unwrap();
        let model = r#"{"parent":"minecraft:block/cube","textures":{"up":"block/stone","down":"block/stone","side":"block/stone"}}"#;
        writer
            .add(
                "assets/minecraft/models/block/stone.json",
                model.as_bytes(),
                CodecChoice::Auto,
            )
            .unwrap();
        writer
            .add(
                "assets/minecraft/blockstates/stone.json",
                br#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#,
                CodecChoice::Auto,
            )
            .unwrap();
        let mut pack =
            PackReader::open(Cursor::new(writer.finish().unwrap().0.into_inner())).unwrap();
        let _ = &mut pack;
        let error = build(&mut pack).err().expect("dirt has no blockstate here");
        assert!(matches!(error, SceneError::Registry(_)), "{error}");
    }

    /// Run with `MCJAVAPSV_REAL_PACK=path/to/data.pak cargo test -- --ignored`. It uses your own
    /// baked game data, and prints only counts.
    #[test]
    #[ignore = "needs a data pack baked from your own copy of the game"]
    fn the_world_builds_from_a_real_data_pack() {
        let path = std::env::var("MCJAVAPSV_REAL_PACK").expect("set MCJAVAPSV_REAL_PACK");
        let mut pack = PackReader::open(std::fs::File::open(path).unwrap()).unwrap();
        let started = std::time::Instant::now();
        let scene = build(&mut pack).unwrap();
        println!(
            "real world: {} textures, atlas {}x{}, {} sections, {} quads, {} vertices, {} ms",
            scene.stats.textures,
            scene.atlas.width,
            scene.atlas.height,
            scene.stats.sections,
            scene.stats.quads,
            scene.stats.vertices,
            started.elapsed().as_millis()
        );
        assert!(scene.stats.quads > 1000);
    }
}
