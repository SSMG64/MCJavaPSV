//! Blockstate JSON: which model, and which rotation, a block with given properties uses.

use std::collections::BTreeMap;

use serde_json::Value;
use thiserror::Error;

use crate::geometry::Rotation;

#[derive(Debug, Error)]
pub enum BlockstateError {
    #[error("the blockstate is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid blockstate: {0}")]
    Invalid(String),
}

fn invalid(message: impl Into<String>) -> BlockstateError {
    BlockstateError::Invalid(message.into())
}

pub type Properties = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub model: String,
    pub rotation: Rotation,
    pub uvlock: bool,
    pub weight: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// Every property must be one of its listed values.
    Match(Vec<(String, Vec<String>)>),
    Or(Vec<Condition>),
    And(Vec<Condition>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub when: Option<Condition>,
    pub apply: Vec<Variant>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Blockstate {
    Variants(Vec<(Properties, Vec<Variant>)>),
    Multipart(Vec<Part>),
}

fn parse_variant(value: &Value) -> Result<Variant, BlockstateError> {
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("a variant has no model"))?
        .to_owned();
    let degrees = |key: &str| value.get(key).and_then(Value::as_i64).unwrap_or(0) as i32;
    let rotation = Rotation::from_degrees(degrees("x"), degrees("y"))
        .ok_or_else(|| invalid("a variant rotation is not a multiple of 90"))?;
    Ok(Variant {
        model,
        rotation,
        uvlock: value
            .get("uvlock")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        weight: value
            .get("weight")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1) as u32,
    })
}

/// A variant entry is one variant or a weighted list of them.
fn parse_variants(value: &Value) -> Result<Vec<Variant>, BlockstateError> {
    match value {
        Value::Array(list) => list.iter().map(parse_variant).collect(),
        other => Ok(vec![parse_variant(other)?]),
    }
}

fn parse_key(key: &str) -> Result<Properties, BlockstateError> {
    let mut out = Properties::new();
    if key.is_empty() || key == "normal" {
        return Ok(out);
    }
    for pair in key.split(',') {
        let (name, value) = pair
            .split_once('=')
            .ok_or_else(|| invalid(format!("variant key {key:?} is not name=value pairs")))?;
        out.insert(name.to_owned(), value.to_owned());
    }
    Ok(out)
}

fn parse_condition(value: &Value) -> Result<Condition, BlockstateError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("a condition must be an object"))?;
    for (key, build) in [
        ("OR", Condition::Or as fn(Vec<Condition>) -> Condition),
        ("AND", Condition::And),
    ] {
        if let Some(list) = object.get(key).and_then(Value::as_array) {
            return Ok(build(
                list.iter().map(parse_condition).collect::<Result<_, _>>()?,
            ));
        }
    }
    let mut checks = Vec::new();
    for (name, wanted) in object {
        let text = match wanted {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            _ => return Err(invalid("a condition value must be text")),
        };
        checks.push((name.clone(), text.split('|').map(str::to_owned).collect()));
    }
    Ok(Condition::Match(checks))
}

pub fn parse(json: &str) -> Result<Blockstate, BlockstateError> {
    let root: Value = serde_json::from_str(json)?;
    if let Some(variants) = root.get("variants").and_then(Value::as_object) {
        let mut out = Vec::new();
        for (key, value) in variants {
            out.push((parse_key(key)?, parse_variants(value)?));
        }
        return Ok(Blockstate::Variants(out));
    }
    if let Some(parts) = root.get("multipart").and_then(Value::as_array) {
        let mut out = Vec::new();
        for part in parts {
            out.push(Part {
                when: part.get("when").map(parse_condition).transpose()?,
                apply: parse_variants(
                    part.get("apply")
                        .ok_or_else(|| invalid("a part has no apply"))?,
                )?,
            });
        }
        return Ok(Blockstate::Multipart(out));
    }
    Err(invalid("neither variants nor multipart"))
}

impl Condition {
    fn matches(&self, properties: &Properties) -> bool {
        match self {
            Self::Match(checks) => checks.iter().all(|(name, wanted)| {
                properties
                    .get(name)
                    .is_some_and(|have| wanted.contains(have))
            }),
            Self::Or(list) => list.iter().any(|c| c.matches(properties)),
            Self::And(list) => list.iter().all(|c| c.matches(properties)),
        }
    }
}

/// Chooses among weighted variants. `seed` is a position hash, so the same block always
/// looks the same but neighbours vary.
pub fn pick(variants: &[Variant], seed: u32) -> Option<&Variant> {
    let total: u64 = variants.iter().map(|v| u64::from(v.weight)).sum();
    if total == 0 {
        return None;
    }
    let mut remaining = u64::from(seed) % total;
    variants.iter().find(|v| {
        let weight = u64::from(v.weight);
        if remaining < weight {
            true
        } else {
            remaining -= weight;
            false
        }
    })
}

impl Blockstate {
    /// The variants that apply to a block with these properties: at most one for `variants`,
    /// possibly several for `multipart`.
    pub fn select<'a>(&'a self, properties: &Properties, seed: u32) -> Vec<&'a Variant> {
        match self {
            Self::Variants(entries) => entries
                .iter()
                .find(|(key, _)| key.iter().all(|(n, v)| properties.get(n) == Some(v)))
                .and_then(|(_, variants)| pick(variants, seed))
                .into_iter()
                .collect(),
            Self::Multipart(parts) => parts
                .iter()
                .filter(|part| part.when.as_ref().is_none_or(|c| c.matches(properties)))
                .filter_map(|part| pick(&part.apply, seed))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &[(&str, &str)]) -> Properties {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn variants_pick_the_matching_key_and_rotation() {
        let state = parse(
            r#"{"variants":{
                "facing=north":{"model":"block/furnace"},
                "facing=east":{"model":"block/furnace","y":90},
                "facing=west,lit=true":{"model":"block/furnace_on","y":270,"uvlock":true}}}"#,
        )
        .unwrap();
        let east = state.select(&props(&[("facing", "east"), ("lit", "false")]), 0);
        assert_eq!(east[0].model, "block/furnace");
        assert_eq!(east[0].rotation, Rotation::from_degrees(0, 90).unwrap());
        let west = state.select(&props(&[("facing", "west"), ("lit", "true")]), 0);
        assert_eq!(west[0].model, "block/furnace_on");
        assert!(west[0].uvlock);
        assert!(
            state
                .select(&props(&[("facing", "west"), ("lit", "false")]), 0)
                .is_empty()
        );
    }

    #[test]
    fn an_empty_key_matches_everything() {
        let state = parse(r#"{"variants":{"":{"model":"block/stone"}}}"#).unwrap();
        assert_eq!(state.select(&props(&[("anything", "x")]), 0).len(), 1);
        let normal = parse(r#"{"variants":{"normal":{"model":"block/a"}}}"#).unwrap();
        assert_eq!(normal.select(&Properties::new(), 0).len(), 1);
    }

    #[test]
    fn weighted_lists_are_chosen_by_seed() {
        let state = parse(
            r#"{"variants":{"":[{"model":"a","weight":3},{"model":"b"},{"model":"c","y":180}]}}"#,
        )
        .unwrap();
        let model = |seed| state.select(&Properties::new(), seed)[0].model.clone();
        assert_eq!(
            [model(0), model(2), model(3), model(4), model(5)],
            ["a", "a", "b", "c", "a"]
        );
    }

    #[test]
    fn multipart_applies_every_matching_part() {
        let state = parse(
            r#"{"multipart":[
                {"apply":{"model":"fence_post"}},
                {"when":{"north":"true"},"apply":{"model":"fence_side"}},
                {"when":{"east":"true"},"apply":{"model":"fence_side","y":90}},
                {"when":{"OR":[{"south":"true"},{"west":"true|maybe"}]},"apply":{"model":"fence_side","y":180}}]}"#,
        )
        .unwrap();
        let none = state.select(&props(&[("north", "false")]), 0);
        assert_eq!(none.len(), 1);
        let some = state.select(
            &props(&[("north", "true"), ("east", "true"), ("west", "maybe")]),
            0,
        );
        let models: Vec<_> = some
            .iter()
            .map(|v| (v.model.as_str(), v.rotation.y_turns))
            .collect();
        assert_eq!(
            models,
            [
                ("fence_post", 0),
                ("fence_side", 0),
                ("fence_side", 1),
                ("fence_side", 2)
            ]
        );
    }

    #[test]
    fn and_conditions_need_every_branch() {
        let state = parse(
            r#"{"multipart":[{"when":{"AND":[{"a":"1"},{"b":"2"}]},"apply":{"model":"m"}}]}"#,
        )
        .unwrap();
        assert!(state.select(&props(&[("a", "1")]), 0).is_empty());
        assert_eq!(state.select(&props(&[("a", "1"), ("b", "2")]), 0).len(), 1);
    }

    #[test]
    fn boolean_and_numeric_condition_values_are_accepted() {
        let state =
            parse(r#"{"multipart":[{"when":{"lit":true,"level":3},"apply":{"model":"m"}}]}"#)
                .unwrap();
        assert_eq!(
            state
                .select(&props(&[("lit", "true"), ("level", "3")]), 0)
                .len(),
            1
        );
    }

    #[test]
    fn rejects_malformed_blockstates() {
        assert!(parse("{ nope").is_err());
        assert!(parse("{}").is_err());
        assert!(parse(r#"{"variants":{"x":{"y":90}}}"#).is_err());
        assert!(parse(r#"{"variants":{"x":{"model":"m"}}}"#).is_err());
        assert!(parse(r#"{"variants":{"":{"model":"m","y":45}}}"#).is_err());
        assert!(parse(r#"{"multipart":[{"when":{"a":[1]},"apply":{"model":"m"}}]}"#).is_err());
    }
}
