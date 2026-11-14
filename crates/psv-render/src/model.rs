//! Block model JSON: parsing, parent resolution and texture variables.

use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Seek};

use psv_pack::PackReader;
use serde_json::Value;
use thiserror::Error;

use crate::geometry::Direction;

const MAX_PARENT_DEPTH: usize = 32;

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("the model is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid model: {0}")]
    Invalid(String),
    #[error("model {0:?} was not found")]
    Missing(String),
    #[error("the parent chain of {0:?} loops or is too deep")]
    ParentLoop(String),
}

fn invalid(message: impl Into<String>) -> ModelError {
    ModelError::Invalid(message.into())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub uv: Option<[f32; 4]>,
    pub texture: String,
    pub cull: Option<Direction>,
    /// Quarter turns of the texture on the face.
    pub rotation: u8,
    pub tint: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElementRotation {
    Axis {
        origin: [f32; 3],
        axis: usize,
        degrees: f32,
        rescale: bool,
    },
    Euler {
        origin: [f32; 3],
        degrees: [f32; 3],
        rescale: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub rotation: Option<ElementRotation>,
    pub shade: bool,
    pub faces: [Option<Face>; 6],
}

/// One entry of a model's `textures`. It is either a plain texture location, or an object
/// that also says the texture must be drawn blended even if its pixels look opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureSlot {
    pub sprite: String,
    pub force_translucent: bool,
}

impl TextureSlot {
    fn plain(sprite: &str) -> Self {
        Self {
            sprite: sprite.to_owned(),
            force_translucent: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub parent: Option<String>,
    pub textures: BTreeMap<String, TextureSlot>,
    pub elements: Option<Vec<Element>>,
    pub ambient_occlusion: Option<bool>,
}

fn vec3(value: &Value, what: &str) -> Result<[f32; 3], ModelError> {
    let array = value
        .as_array()
        .filter(|a| a.len() == 3)
        .ok_or_else(|| invalid(format!("{what} must be three numbers")))?;
    let mut out = [0.0; 3];
    for (slot, item) in out.iter_mut().zip(array) {
        *slot = item
            .as_f64()
            .ok_or_else(|| invalid(format!("{what} must be three numbers")))?
            as f32;
    }
    Ok(out)
}

fn parse_rotation(value: &Value) -> Result<ElementRotation, ModelError> {
    let origin = value
        .get("origin")
        .map_or(Ok([8.0; 3]), |o| vec3(o, "rotation origin"))?;
    let rescale = value
        .get("rescale")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if let Some(axis) = value.get("axis").and_then(Value::as_str) {
        let axis = match axis {
            "x" => 0,
            "y" => 1,
            "z" => 2,
            other => return Err(invalid(format!("unknown rotation axis {other:?}"))),
        };
        let degrees = value.get("angle").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        return Ok(ElementRotation::Axis {
            origin,
            axis,
            degrees,
            rescale,
        });
    }
    let angle = |key: &str| value.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    Ok(ElementRotation::Euler {
        origin,
        degrees: [angle("x"), angle("y"), angle("z")],
        rescale,
    })
}

fn parse_face(value: &Value) -> Result<Face, ModelError> {
    let texture = value
        .get("texture")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("a face has no texture"))?
        .to_owned();
    let uv = match value.get("uv") {
        None => None,
        Some(uv) => {
            let a = uv
                .as_array()
                .filter(|a| a.len() == 4)
                .ok_or_else(|| invalid("uv must be four numbers"))?;
            let mut out = [0.0; 4];
            for (slot, item) in out.iter_mut().zip(a) {
                *slot = item
                    .as_f64()
                    .ok_or_else(|| invalid("uv must be four numbers"))?
                    as f32;
            }
            Some(out)
        }
    };
    let degrees = value.get("rotation").and_then(Value::as_i64).unwrap_or(0);
    if degrees % 90 != 0 {
        return Err(invalid(format!(
            "a face rotation of {degrees} is not a multiple of 90"
        )));
    }
    Ok(Face {
        uv,
        texture,
        cull: value
            .get("cullface")
            .and_then(Value::as_str)
            .and_then(Direction::parse),
        rotation: (degrees / 90).rem_euclid(4) as u8,
        tint: value.get("tintindex").and_then(Value::as_i64).unwrap_or(-1) as i32,
    })
}

fn parse_element(value: &Value) -> Result<Element, ModelError> {
    let from = vec3(
        value
            .get("from")
            .ok_or_else(|| invalid("an element has no from"))?,
        "from",
    )?;
    let to = vec3(
        value
            .get("to")
            .ok_or_else(|| invalid("an element has no to"))?,
        "to",
    )?;
    let mut faces: [Option<Face>; 6] = Default::default();
    if let Some(map) = value.get("faces").and_then(Value::as_object) {
        for (name, face) in map {
            if let Some(direction) = Direction::parse(name) {
                faces[direction.index()] = Some(parse_face(face)?);
            }
        }
    }
    Ok(Element {
        from,
        to,
        rotation: value.get("rotation").map(parse_rotation).transpose()?,
        shade: value.get("shade").and_then(Value::as_bool).unwrap_or(true),
        faces,
    })
}

pub fn parse_model(json: &str) -> Result<Model, ModelError> {
    let root: Value = serde_json::from_str(json)?;
    let object = root
        .as_object()
        .ok_or_else(|| invalid("a model must be an object"))?;
    let mut textures = BTreeMap::new();
    if let Some(map) = object.get("textures").and_then(Value::as_object) {
        for (key, value) in map {
            if let Some(text) = value.as_str() {
                textures.insert(key.clone(), TextureSlot::plain(text));
            } else if let Some(sprite) = value.get("sprite").and_then(Value::as_str) {
                textures.insert(
                    key.clone(),
                    TextureSlot {
                        sprite: sprite.to_owned(),
                        force_translucent: value
                            .get("force_translucent")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    },
                );
            }
        }
    }
    let elements = match object.get("elements").and_then(Value::as_array) {
        None => None,
        Some(list) => Some(
            list.iter()
                .map(parse_element)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    };
    Ok(Model {
        parent: object
            .get("parent")
            .and_then(Value::as_str)
            .map(str::to_owned),
        textures,
        elements,
        ambient_occlusion: object.get("ambientocclusion").and_then(Value::as_bool),
    })
}

/// `block/stone` is `minecraft:block/stone`.
pub fn normalize_location(location: &str) -> String {
    if location.contains(':') {
        location.to_owned()
    } else {
        format!("minecraft:{location}")
    }
}

/// Where the JSON for a model comes from.
pub trait ModelSource {
    fn load(&mut self, location: &str) -> Option<String>;
}

/// Reads `assets/<namespace>/models/<path>.json` from a data pack.
pub struct PackModels<'a, R: Read + Seek> {
    pub pack: &'a mut PackReader<R>,
}

impl<R: Read + Seek> ModelSource for PackModels<'_, R> {
    fn load(&mut self, location: &str) -> Option<String> {
        let (namespace, path) = normalize_location(location)
            .split_once(':')
            .map(|(a, b)| (a.to_owned(), b.to_owned()))?;
        let bytes = self
            .pack
            .read(&format!("assets/{namespace}/models/{path}.json"))
            .ok()?;
        String::from_utf8(bytes).ok()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedModel {
    pub textures: BTreeMap<String, TextureSlot>,
    pub elements: Vec<Element>,
    pub ambient_occlusion: bool,
    /// A `builtin/...` parent: drawn by special code, with no elements of its own.
    pub builtin: bool,
}

fn is_builtin(location: &str) -> bool {
    let rest = location.strip_prefix("minecraft:").unwrap_or(location);
    rest.starts_with("builtin/")
}

/// Loads a model and its parents. Children override the parents' texture variables, and the
/// nearest model that lists elements supplies them.
pub fn resolve(location: &str, source: &mut dyn ModelSource) -> Result<ResolvedModel, ModelError> {
    let mut chain: Vec<Model> = Vec::new();
    let mut seen = HashSet::new();
    let mut builtin = false;
    let mut next = Some(normalize_location(location));
    while let Some(current) = next.take() {
        if chain.len() >= MAX_PARENT_DEPTH || !seen.insert(current.clone()) {
            return Err(ModelError::ParentLoop(current));
        }
        if is_builtin(&current) {
            builtin = true;
            break;
        }
        let json = source
            .load(&current)
            .ok_or_else(|| ModelError::Missing(current.clone()))?;
        let model = parse_model(&json)?;
        next = model.parent.as_deref().map(normalize_location);
        chain.push(model);
    }

    let mut textures = BTreeMap::new();
    for model in chain.iter().rev() {
        textures.extend(model.textures.clone());
    }
    let elements = chain
        .iter()
        .find_map(|m| m.elements.clone())
        .unwrap_or_default();
    let ambient_occlusion = chain
        .iter()
        .find_map(|m| m.ambient_occlusion)
        .unwrap_or(true);
    Ok(ResolvedModel {
        textures,
        elements,
        ambient_occlusion,
        builtin,
    })
}

/// A texture reference followed to its end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTexture {
    pub location: String,
    pub force_translucent: bool,
}

impl ResolvedModel {
    /// Follows texture slots until a real texture location comes out. A name is looked up as a
    /// slot whether or not it starts with `#`, as the game does; a name that is no slot is taken
    /// as a location. Any slot on the way that forces translucency makes the result translucent.
    pub fn resolve_texture(&self, reference: &str) -> Option<ResolvedTexture> {
        let mut current = reference;
        let mut force_translucent = false;
        for _ in 0..MAX_PARENT_DEPTH {
            let name = current.strip_prefix('#');
            let slot = self.textures.get(name.unwrap_or(current));
            match (slot, name) {
                (Some(slot), _) => {
                    force_translucent |= slot.force_translucent;
                    current = &slot.sprite;
                }
                (None, Some(_)) => return None,
                (None, None) => {
                    return Some(ResolvedTexture {
                        location: normalize_location(current),
                        force_translucent,
                    });
                }
            }
        }
        None
    }

    pub fn texture(&self, reference: &str) -> Option<String> {
        self.resolve_texture(reference).map(|t| t.location)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    struct Fake(HashMap<String, String>);

    impl ModelSource for Fake {
        fn load(&mut self, location: &str) -> Option<String> {
            self.0.get(location).cloned()
        }
    }

    fn fake(entries: &[(&str, &str)]) -> Fake {
        Fake(
            entries
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        )
    }

    const CUBE: &str = r##"{
        "textures": {"particle": "#all"},
        "elements": [{
            "from": [0, 0, 0], "to": [16, 16, 16],
            "faces": {
                "down": {"texture": "#all", "cullface": "down"},
                "up": {"texture": "#all", "cullface": "up", "tintindex": 0},
                "north": {"uv": [0, 0, 8, 8], "texture": "#all", "cullface": "north", "rotation": 90}
            }
        }]
    }"##;

    #[test]
    fn parses_elements_faces_and_defaults() {
        let model = parse_model(CUBE).unwrap();
        let element = &model.elements.unwrap()[0];
        assert_eq!(element.from, [0.0; 3]);
        assert_eq!(element.to, [16.0; 3]);
        assert!(element.shade);
        let up = element.faces[Direction::Up.index()].as_ref().unwrap();
        assert_eq!((up.tint, up.rotation, up.uv), (0, 0, None));
        let north = element.faces[Direction::North.index()].as_ref().unwrap();
        assert_eq!((north.rotation, north.uv), (1, Some([0.0, 0.0, 8.0, 8.0])));
        assert_eq!(north.cull, Some(Direction::North));
        assert!(element.faces[Direction::East.index()].is_none());
    }

    #[test]
    fn parses_both_rotation_styles() {
        let axis = parse_rotation(
            &serde_json::json!({"origin":[8,0,8],"axis":"y","angle":45,"rescale":true}),
        )
        .unwrap();
        assert_eq!(
            axis,
            ElementRotation::Axis {
                origin: [8.0, 0.0, 8.0],
                axis: 1,
                degrees: 45.0,
                rescale: true
            }
        );
        let euler = parse_rotation(&serde_json::json!({"x":10,"z":-5})).unwrap();
        assert_eq!(
            euler,
            ElementRotation::Euler {
                origin: [8.0; 3],
                degrees: [10.0, 0.0, -5.0],
                rescale: false
            }
        );
        assert!(parse_rotation(&serde_json::json!({"axis":"w"})).is_err());
    }

    #[test]
    fn rejects_broken_models() {
        assert!(matches!(parse_model("{ nope"), Err(ModelError::Json(_))));
        assert!(parse_model("[]").is_err());
        assert!(parse_model(r#"{"elements":[{"to":[1,1,1]}]}"#).is_err());
        assert!(parse_model(r#"{"elements":[{"from":[0,0],"to":[1,1,1]}]}"#).is_err());
        let bad_rotation = r##"{"elements":[{"from":[0,0,0],"to":[1,1,1],"faces":{"up":{"texture":"#a","rotation":45}}}]}"##;
        assert!(parse_model(bad_rotation).is_err());
    }

    #[test]
    fn children_override_textures_and_inherit_elements() {
        let mut source = fake(&[
            (
                "minecraft:block/cube_all",
                r##"{"parent":"minecraft:block/cube","textures":{"particle":"#all"}}"##,
            ),
            ("minecraft:block/cube", CUBE),
            (
                "minecraft:block/stone",
                r#"{"parent":"block/cube_all","textures":{"all":"block/stone"}}"#,
            ),
        ]);
        let model = resolve("block/stone", &mut source).unwrap();
        assert_eq!(model.elements.len(), 1);
        assert_eq!(
            model.texture("#all").as_deref(),
            Some("minecraft:block/stone")
        );
        assert_eq!(
            model.texture("#particle").as_deref(),
            Some("minecraft:block/stone")
        );
        assert_eq!(model.texture("mymod:thing").as_deref(), Some("mymod:thing"));
        assert_eq!(model.texture("#nothing"), None);
        assert!(model.ambient_occlusion && !model.builtin);
    }

    #[test]
    fn a_child_that_lists_elements_replaces_the_parents() {
        let mut source = fake(&[
            (
                "minecraft:a",
                r#"{"parent":"minecraft:b","elements":[{"from":[0,0,0],"to":[1,1,1]}]}"#,
            ),
            (
                "minecraft:b",
                r#"{"elements":[{"from":[0,0,0],"to":[2,2,2]},{"from":[0,0,0],"to":[3,3,3]}]}"#,
            ),
        ]);
        assert_eq!(resolve("a", &mut source).unwrap().elements.len(), 1);
    }

    #[test]
    fn texture_variable_loops_and_parent_loops_are_detected() {
        let mut source = fake(&[
            (
                "minecraft:a",
                r##"{"parent":"minecraft:b","textures":{"x":"#y","y":"#x"}}"##,
            ),
            ("minecraft:b", r#"{"parent":"minecraft:a"}"#),
        ]);
        assert!(matches!(
            resolve("a", &mut source),
            Err(ModelError::ParentLoop(_))
        ));
        let looped = ResolvedModel {
            textures: [
                ("x".to_owned(), TextureSlot::plain("#y")),
                ("y".to_owned(), TextureSlot::plain("#x")),
            ]
            .into(),
            elements: vec![],
            ambient_occlusion: true,
            builtin: false,
        };
        assert_eq!(looped.texture("#x"), None);
    }

    #[test]
    fn missing_parents_and_builtin_parents() {
        let mut source = fake(&[
            ("minecraft:a", r#"{"parent":"minecraft:gone"}"#),
            ("minecraft:chest", r#"{"parent":"builtin/entity"}"#),
        ]);
        assert!(
            matches!(resolve("a", &mut source), Err(ModelError::Missing(m)) if m == "minecraft:gone")
        );
        let chest = resolve("chest", &mut source).unwrap();
        assert!(chest.builtin && chest.elements.is_empty());
    }

    #[test]
    fn locations_get_the_default_namespace() {
        assert_eq!(normalize_location("block/stone"), "minecraft:block/stone");
        assert_eq!(normalize_location("mod:block/x"), "mod:block/x");
    }

    #[test]
    fn texture_slots_can_be_objects_that_force_translucency() {
        let mut source = fake(&[
            (
                "minecraft:pane",
                r##"{"parent":"minecraft:cube","textures":{
                    "edge":{"sprite":"minecraft:block/glass_top","force_translucent":true},
                    "pane":{"sprite":"#edge"},
                    "plain":"block/stone",
                    "broken":{"force_translucent":true}}}"##,
            ),
            ("minecraft:cube", CUBE),
        ]);
        let model = resolve("pane", &mut source).unwrap();
        let edge = model.resolve_texture("#edge").unwrap();
        assert_eq!(edge.location, "minecraft:block/glass_top");
        assert!(edge.force_translucent);
        let through_pane = model.resolve_texture("#pane").unwrap();
        assert!(
            through_pane.force_translucent,
            "the flag survives a #reference"
        );
        assert_eq!(through_pane.location, "minecraft:block/glass_top");
        assert!(!model.resolve_texture("#plain").unwrap().force_translucent);
        assert_eq!(model.resolve_texture("#broken"), None);
        assert_eq!(
            model.texture("#plain").as_deref(),
            Some("minecraft:block/stone")
        );
    }

    #[test]
    fn a_name_without_a_hash_is_still_looked_up_as_a_slot_first() {
        let model = ResolvedModel {
            textures: [("all".to_owned(), TextureSlot::plain("block/heavy"))].into(),
            elements: vec![],
            ambient_occlusion: true,
            builtin: false,
        };
        assert_eq!(
            model.texture("all").as_deref(),
            Some("minecraft:block/heavy")
        );
        assert_eq!(
            model.texture("#all").as_deref(),
            Some("minecraft:block/heavy")
        );
        assert_eq!(
            model.texture("block/stone").as_deref(),
            Some("minecraft:block/stone")
        );
        assert_eq!(model.texture("#nothing"), None);
    }
}
