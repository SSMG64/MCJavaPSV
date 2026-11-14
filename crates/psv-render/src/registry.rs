//! From a block name and its properties to the quads that draw it, using a data pack.

use std::io::{Read, Seek};

use psv_pack::{PackError, PackReader};
use thiserror::Error;

use crate::blockstate::{self, BlockstateError, Properties};
use crate::mesh::Appearance;
use crate::model::{self, ModelError, PackModels, ResolvedModel, normalize_location};
use crate::quad::{BakeReport, Sprites, bake_locked};

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("there is no blockstate for {0:?}")]
    NoBlockstate(String),
    #[error("blockstate of {name}: {source}")]
    Blockstate {
        name: String,
        source: BlockstateError,
    },
    #[error("model of {name}: {source}")]
    Model { name: String, source: ModelError },
    #[error("could not read the pack: {0}")]
    Pack(#[from] PackError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuiltBlock {
    pub appearance: Appearance,
    pub report: BakeReport,
}

/// Builds the appearance of `name` with `properties`. `seed` chooses among weighted variants.
/// A block whose properties select nothing has an empty appearance, which draws nothing.
pub fn build_appearance<R: Read + Seek>(
    pack: &mut PackReader<R>,
    sprites: &dyn Sprites,
    name: &str,
    properties: &Properties,
    seed: u32,
) -> Result<BuiltBlock, RegistryError> {
    let location = normalize_location(name);
    let (namespace, block) = location.split_once(':').unwrap_or(("minecraft", &location));
    let path = format!("assets/{namespace}/blockstates/{block}.json");
    if !pack.contains(&path) {
        return Err(RegistryError::NoBlockstate(location));
    }
    let json = String::from_utf8_lossy(&pack.read(&path)?).into_owned();
    let state = blockstate::parse(&json).map_err(|source| RegistryError::Blockstate {
        name: location.clone(),
        source,
    })?;

    let mut quads = Vec::new();
    let mut report = BakeReport::default();
    for variant in state.select(properties, seed) {
        let model: ResolvedModel = model::resolve(&variant.model, &mut PackModels { pack })
            .map_err(|source| RegistryError::Model {
                name: location.clone(),
                source,
            })?;
        let (baked, part) = bake_locked(&model, variant.rotation, variant.uvlock, sprites);
        quads.extend(baked);
        report
            .unresolved_variables
            .extend(part.unresolved_variables);
        report.missing_sprites.extend(part.missing_sprites);
    }
    Ok(BuiltBlock {
        appearance: Appearance::new(quads),
        report,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use psv_pack::{CodecChoice, PackWriter};

    use super::*;
    use crate::atlas::{Atlas, AtlasEntry};
    use crate::geometry::Direction;

    const CUBE: &str = r##"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{
        "down":{"texture":"#all","cullface":"down"},"up":{"texture":"#all","cullface":"up"},
        "north":{"texture":"#all","cullface":"north"},"south":{"texture":"#all","cullface":"south"},
        "west":{"texture":"#all","cullface":"west"},"east":{"texture":"#all","cullface":"east"}}}]}"##;

    fn pack(files: &[(&str, &str)]) -> PackReader<Cursor<Vec<u8>>> {
        let mut writer = PackWriter::new(Cursor::new(Vec::new())).unwrap();
        for (path, text) in files {
            writer
                .add(path, text.as_bytes(), CodecChoice::Auto)
                .unwrap();
        }
        PackReader::open(Cursor::new(writer.finish().unwrap().0.into_inner())).unwrap()
    }

    fn atlas() -> Atlas {
        let entry = |name: &str| AtlasEntry {
            name: name.into(),
            width: 4,
            height: 4,
            rgba: [9, 9, 9, 255].repeat(16),
            flags: 0,
        };
        Atlas::build(
            vec![entry("minecraft:block/stone"), entry("minecraft:block/log")],
            1,
            64,
        )
        .unwrap()
    }

    fn props(pairs: &[(&str, &str)]) -> Properties {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn builds_a_plain_cube_from_blockstate_and_model_files() {
        let mut pack = pack(&[
            (
                "assets/minecraft/blockstates/stone.json",
                r#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#,
            ),
            (
                "assets/minecraft/models/block/stone.json",
                r#"{"parent":"block/cube_all","textures":{"all":"block/stone"}}"#,
            ),
            ("assets/minecraft/models/block/cube_all.json", CUBE),
        ]);
        let built = build_appearance(&mut pack, &atlas(), "stone", &Properties::new(), 0).unwrap();
        assert_eq!(built.appearance.quads.len(), 6);
        assert_eq!(built.appearance.occludes, [true; 6]);
        assert_eq!(built.report, BakeReport::default());
    }

    #[test]
    fn properties_choose_the_variant_and_its_rotation() {
        let mut pack = pack(&[
            (
                "assets/minecraft/blockstates/log.json",
                r#"{"variants":{
                    "axis=y":{"model":"minecraft:block/log"},
                    "axis=x":{"model":"minecraft:block/log","x":90,"y":90}}}"#,
            ),
            (
                "assets/minecraft/models/block/log.json",
                r#"{"parent":"block/cube_all","textures":{"all":"block/log"}}"#,
            ),
            ("assets/minecraft/models/block/cube_all.json", CUBE),
        ]);
        let upright = build_appearance(
            &mut pack,
            &atlas(),
            "minecraft:log",
            &props(&[("axis", "y")]),
            0,
        )
        .unwrap();
        let sideways =
            build_appearance(&mut pack, &atlas(), "log", &props(&[("axis", "x")]), 0).unwrap();
        assert_eq!(
            upright.appearance.quads.len(),
            sideways.appearance.quads.len()
        );
        let face_of = |built: &BuiltBlock, cull| {
            built
                .appearance
                .quads
                .iter()
                .find(|q| q.cull == Some(cull))
                .map(|q| q.face)
        };
        assert_eq!(face_of(&upright, Direction::Up), Some(Direction::Up));
        assert!(sideways.appearance.quads.iter().all(|q| q.cull.is_some()));
    }

    #[test]
    fn properties_that_select_nothing_draw_nothing() {
        let mut pack = pack(&[(
            "assets/minecraft/blockstates/stone.json",
            r#"{"variants":{"lit=true":{"model":"minecraft:block/stone"}}}"#,
        )]);
        let built =
            build_appearance(&mut pack, &atlas(), "stone", &props(&[("lit", "false")]), 0).unwrap();
        assert!(built.appearance.quads.is_empty());
    }

    #[test]
    fn missing_pieces_are_reported() {
        let mut pack = pack(&[
            (
                "assets/minecraft/blockstates/odd.json",
                r#"{"variants":{"":{"model":"minecraft:block/gone"}}}"#,
            ),
            ("assets/minecraft/blockstates/bad.json", "{ not json"),
            (
                "assets/minecraft/blockstates/lost.json",
                r#"{"variants":{"":{"model":"minecraft:block/lost"}}}"#,
            ),
            (
                "assets/minecraft/models/block/lost.json",
                r##"{"textures":{"all":"block/nowhere"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
            ),
        ]);
        let none = Properties::new();
        assert!(matches!(
            build_appearance(&mut pack, &atlas(), "air", &none, 0),
            Err(RegistryError::NoBlockstate(_))
        ));
        assert!(matches!(
            build_appearance(&mut pack, &atlas(), "odd", &none, 0),
            Err(RegistryError::Model { .. })
        ));
        assert!(matches!(
            build_appearance(&mut pack, &atlas(), "bad", &none, 0),
            Err(RegistryError::Blockstate { .. })
        ));
        let lost = build_appearance(&mut pack, &atlas(), "lost", &none, 0).unwrap();
        assert!(lost.appearance.quads.is_empty());
        assert_eq!(lost.report.missing_sprites, ["minecraft:block/nowhere"]);
    }
}
