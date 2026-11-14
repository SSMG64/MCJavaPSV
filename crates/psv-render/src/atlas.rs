use std::collections::HashMap;
use std::io::{Read, Seek};

use psv_pack::{PackError, PackReader, texture};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AtlasError {
    #[error("{0:?} appears twice")]
    Duplicate(String),
    #[error("the textures need more than {max}x{max} pixels")]
    TooLarge { max: u32 },
    #[error("{name:?} has no pixels or the wrong amount")]
    BadTexture { name: String },
    #[error("could not read the pack: {0}")]
    Pack(#[from] PackError),
}

#[derive(Debug, Clone)]
pub struct AtlasEntry {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub flags: u8,
}

/// Where one texture sits in the atlas, in pixels, without its padding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sprite {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub flags: u8,
}

/// The same rectangle as fractions of the atlas, ready to use as texture coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvRect {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

#[derive(Debug, Clone)]
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    sprites: HashMap<String, Sprite>,
}

struct Placement {
    entry: usize,
    x: u32,
    y: u32,
}

/// Shelf packing: fills rows left to right, starting a new row when one is full. Returns the
/// placements and the height used, or `None` when a texture is wider than the atlas.
fn shelf_pack(order: &[(usize, u32, u32)], width: u32) -> Option<(Vec<Placement>, u32)> {
    let mut placements = Vec::with_capacity(order.len());
    let (mut x, mut y, mut row_height) = (0, 0, 0);
    for &(entry, w, h) in order {
        if w > width {
            return None;
        }
        if x + w > width {
            y += row_height;
            x = 0;
            row_height = 0;
        }
        placements.push(Placement { entry, x, y });
        x += w;
        row_height = row_height.max(h);
    }
    Some((placements, y + row_height))
}

impl Atlas {
    /// Packs the textures into the smallest power of two atlas that fits within `max_size`.
    /// Each texture gets `padding` pixels of its own edge colors around it, which keeps
    /// filtering from bleeding a neighbour into it.
    pub fn build(
        entries: Vec<AtlasEntry>,
        padding: u32,
        max_size: u32,
    ) -> Result<Atlas, AtlasError> {
        let mut seen = HashMap::new();
        for (index, entry) in entries.iter().enumerate() {
            if entry.width == 0
                || entry.height == 0
                || entry.rgba.len() != entry.width as usize * entry.height as usize * 4
            {
                return Err(AtlasError::BadTexture {
                    name: entry.name.clone(),
                });
            }
            if seen.insert(entry.name.as_str(), index).is_some() {
                return Err(AtlasError::Duplicate(entry.name.clone()));
            }
        }

        let mut order: Vec<(usize, u32, u32)> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (i, e.width + 2 * padding, e.height + 2 * padding))
            .collect();
        order.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then(b.1.cmp(&a.1))
                .then(entries[a.0].name.cmp(&entries[b.0].name))
        });

        let area: u64 = order
            .iter()
            .map(|&(_, w, h)| u64::from(w) * u64::from(h))
            .sum();
        let mut best: Option<(u64, u32, u32, Vec<Placement>)> = None;
        let mut width = 16_u32;
        while width <= max_size {
            if let Some((placements, used)) = shelf_pack(&order, width) {
                let height = used.max(1).next_power_of_two();
                if height <= max_size {
                    let size = u64::from(width) * u64::from(height);
                    let stretch = width.max(height) / width.min(height);
                    let better = |b: &(u64, u32, u32, Vec<Placement>)| {
                        let current = b.1.max(b.2) / b.1.min(b.2);
                        (size, stretch) < (b.0, current)
                    };
                    if best.as_ref().is_none_or(better) {
                        best = Some((size, width, height, placements));
                    }
                }
            }
            width *= 2;
        }
        let _ = area;
        let (_, width, height, placements) = best.ok_or(AtlasError::TooLarge { max: max_size })?;

        let mut rgba = vec![0_u8; width as usize * height as usize * 4];
        let mut sprites = HashMap::with_capacity(entries.len());
        for placement in &placements {
            let entry = &entries[placement.entry];
            let (ox, oy) = (placement.x + padding, placement.y + padding);
            let span_w = entry.width + 2 * padding;
            let span_h = entry.height + 2 * padding;
            for dy in 0..span_h {
                let sy = dy.saturating_sub(padding).min(entry.height - 1);
                for dx in 0..span_w {
                    let sx = dx.saturating_sub(padding).min(entry.width - 1);
                    let from = (sy as usize * entry.width as usize + sx as usize) * 4;
                    let to = ((placement.y + dy) as usize * width as usize
                        + (placement.x + dx) as usize)
                        * 4;
                    rgba[to..to + 4].copy_from_slice(&entry.rgba[from..from + 4]);
                }
            }
            sprites.insert(
                entry.name.clone(),
                Sprite {
                    x: ox,
                    y: oy,
                    width: entry.width,
                    height: entry.height,
                    flags: entry.flags,
                },
            );
        }
        Ok(Atlas {
            width,
            height,
            rgba,
            sprites,
        })
    }

    pub fn sprite(&self, name: &str) -> Option<&Sprite> {
        self.sprites.get(name)
    }

    pub fn len(&self) -> usize {
        self.sprites.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sprites.is_empty()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.sprites.keys().map(String::as_str)
    }

    pub fn uv(&self, name: &str) -> Option<UvRect> {
        let s = self.sprite(name)?;
        let (w, h) = (self.width as f32, self.height as f32);
        Some(UvRect {
            u0: s.x as f32 / w,
            v0: s.y as f32 / h,
            u1: (s.x + s.width) as f32 / w,
            v1: (s.y + s.height) as f32 / h,
        })
    }
}

/// `assets/minecraft/textures/block/stone.tex` is the texture `minecraft:block/stone`.
pub fn texture_name(path: &str) -> Option<String> {
    let rest = path.strip_prefix("assets/")?.strip_suffix(".tex")?;
    let (namespace, rest) = rest.split_once('/')?;
    let location = rest.strip_prefix("textures/")?;
    Some(format!("{namespace}:{location}"))
}

/// Reads every texture whose name `keep` accepts. A taller-than-wide texture with an animation
/// file beside it is a strip of frames, and only its first frame is used.
pub fn entries_from_pack<R: Read + Seek>(
    pack: &mut PackReader<R>,
    keep: impl Fn(&str) -> bool,
) -> Result<Vec<AtlasEntry>, AtlasError> {
    let paths: Vec<String> = pack
        .entries()
        .iter()
        .filter(|e| e.path.ends_with(".tex"))
        .map(|e| e.path.clone())
        .collect();
    let mut entries = Vec::new();
    for path in paths {
        let Some(name) = texture_name(&path) else {
            continue;
        };
        if !keep(&name) {
            continue;
        }
        let blob = pack.read(&path)?;
        let mut tex =
            texture::decode(&blob).map_err(|_| AtlasError::BadTexture { name: name.clone() })?;
        let animation_file = format!("{}.png.mcmeta", path.trim_end_matches(".tex"));
        if tex.height > tex.width && tex.height % tex.width == 0 && pack.contains(&animation_file) {
            tex.rgba
                .truncate(tex.width as usize * tex.width as usize * 4);
            tex.height = tex.width;
            tex.flags = texture::flags_of(&tex.rgba);
        }
        entries.push(AtlasEntry {
            name,
            width: tex.width,
            height: tex.height,
            rgba: tex.rgba,
            flags: tex.flags,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use psv_pack::{CodecChoice, PackWriter};

    use super::*;

    fn solid(name: &str, w: u32, h: u32, color: [u8; 4]) -> AtlasEntry {
        AtlasEntry {
            name: name.into(),
            width: w,
            height: h,
            rgba: color.repeat(w as usize * h as usize),
            flags: 0,
        }
    }

    fn pixel(atlas: &Atlas, x: u32, y: u32) -> [u8; 4] {
        let at = (y as usize * atlas.width as usize + x as usize) * 4;
        [
            atlas.rgba[at],
            atlas.rgba[at + 1],
            atlas.rgba[at + 2],
            atlas.rgba[at + 3],
        ]
    }

    #[test]
    fn every_sprite_holds_its_own_pixels() {
        let entries: Vec<_> = (0..40_u8)
            .map(|i| {
                solid(
                    &format!("minecraft:block/t{i}"),
                    16,
                    16,
                    [i, 255 - i, i / 2, 255],
                )
            })
            .collect();
        let atlas = Atlas::build(entries, 1, 256).unwrap();
        assert_eq!(atlas.len(), 40);
        assert!(atlas.width.is_power_of_two() && atlas.height.is_power_of_two());
        for i in 0..40_u8 {
            let s = atlas.sprite(&format!("minecraft:block/t{i}")).unwrap();
            for (dx, dy) in [(0, 0), (15, 0), (0, 15), (15, 15), (7, 8)] {
                assert_eq!(pixel(&atlas, s.x + dx, s.y + dy), [i, 255 - i, i / 2, 255]);
            }
        }
    }

    #[test]
    fn sprites_never_overlap_including_their_padding() {
        let mut entries = Vec::new();
        for i in 0..30_u32 {
            entries.push(solid(&format!("a{i}"), 16, 16, [1, 1, 1, 255]));
            entries.push(solid(&format!("b{i}"), 32, 8 + i % 5, [2, 2, 2, 255]));
        }
        let atlas = Atlas::build(entries, 1, 512).unwrap();
        let rects: Vec<_> = atlas
            .names()
            .map(|n| {
                let s = atlas.sprite(n).unwrap();
                (s.x - 1, s.y - 1, s.x + s.width + 1, s.y + s.height + 1)
            })
            .collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(a.2 <= atlas.width && a.3 <= atlas.height);
            for b in &rects[i + 1..] {
                let apart = a.2 <= b.0 || b.2 <= a.0 || a.3 <= b.1 || b.3 <= a.1;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn padding_repeats_the_edge_pixels() {
        let mut rgba = Vec::new();
        for y in 0..2_u8 {
            for x in 0..2_u8 {
                rgba.extend_from_slice(&[x * 100, y * 100, 7, 255]);
            }
        }
        let entry = AtlasEntry {
            name: "t".into(),
            width: 2,
            height: 2,
            rgba,
            flags: 0,
        };
        let atlas = Atlas::build(vec![entry], 2, 64).unwrap();
        let s = *atlas.sprite("t").unwrap();
        assert_eq!(pixel(&atlas, s.x - 2, s.y - 2), pixel(&atlas, s.x, s.y));
        assert_eq!(
            pixel(&atlas, s.x + 3, s.y + 3),
            pixel(&atlas, s.x + 1, s.y + 1)
        );
        assert_eq!(pixel(&atlas, s.x - 1, s.y + 1), pixel(&atlas, s.x, s.y + 1));
    }

    #[test]
    fn build_is_deterministic_and_uvs_are_fractions() {
        let make = || {
            Atlas::build(
                (0..10)
                    .map(|i| solid(&format!("t{i}"), 16, 16, [i, 0, 0, 255]))
                    .collect(),
                1,
                128,
            )
            .unwrap()
        };
        let (a, b) = (make(), make());
        assert_eq!(a.rgba, b.rgba);
        let uv = a.uv("t3").unwrap();
        let s = a.sprite("t3").unwrap();
        assert_eq!(uv.u0, s.x as f32 / a.width as f32);
        assert_eq!(uv.v1, (s.y + 16) as f32 / a.height as f32);
        assert!(a.uv("missing").is_none());
    }

    #[test]
    fn rejects_duplicates_bad_textures_and_overflow() {
        assert!(matches!(
            Atlas::build(
                vec![solid("a", 4, 4, [0; 4]), solid("a", 4, 4, [0; 4])],
                0,
                64
            ),
            Err(AtlasError::Duplicate(_))
        ));
        let mut broken = solid("b", 4, 4, [0; 4]);
        broken.rgba.pop();
        assert!(matches!(
            Atlas::build(vec![broken], 0, 64),
            Err(AtlasError::BadTexture { .. })
        ));
        let many: Vec<_> = (0..100)
            .map(|i| solid(&format!("t{i}"), 16, 16, [0; 4]))
            .collect();
        assert!(matches!(
            Atlas::build(many, 1, 64),
            Err(AtlasError::TooLarge { .. })
        ));
        assert!(matches!(
            Atlas::build(vec![solid("wide", 200, 4, [0; 4])], 0, 64),
            Err(AtlasError::TooLarge { .. })
        ));
    }

    #[test]
    fn names_come_from_pack_paths() {
        assert_eq!(
            texture_name("assets/minecraft/textures/block/stone.tex").as_deref(),
            Some("minecraft:block/stone")
        );
        assert_eq!(
            texture_name("assets/mymod/textures/item/x/y.tex").as_deref(),
            Some("mymod:item/x/y")
        );
        assert_eq!(
            texture_name("assets/minecraft/models/block/stone.json"),
            None
        );
        assert_eq!(texture_name("data/minecraft/x.tex"), None);
    }

    #[test]
    fn pack_loading_filters_and_crops_animation_strips() {
        let still: Vec<u8> = [9, 9, 9, 255].repeat(16 * 16);
        let mut strip = [1, 2, 3, 255].repeat(16 * 16);
        strip.extend([200, 0, 0, 128].repeat(16 * 16 * 2));
        let mut writer = PackWriter::new(Cursor::new(Vec::new())).unwrap();
        let add = |w: &mut PackWriter<Cursor<Vec<u8>>>, path: &str, h: u32, px: &[u8]| {
            let blob = texture::encode(16, h, px).unwrap();
            w.add(path, &blob, CodecChoice::Auto).unwrap();
        };
        add(
            &mut writer,
            "assets/minecraft/textures/block/stone.tex",
            16,
            &still,
        );
        add(
            &mut writer,
            "assets/minecraft/textures/block/water_still.tex",
            48,
            &strip,
        );
        add(
            &mut writer,
            "assets/minecraft/textures/item/apple.tex",
            16,
            &still,
        );
        writer
            .add(
                "assets/minecraft/textures/block/water_still.png.mcmeta",
                b"{}",
                CodecChoice::Store,
            )
            .unwrap();
        let bytes = writer.finish().unwrap().0.into_inner();
        let mut pack = PackReader::open(Cursor::new(bytes)).unwrap();

        let entries = entries_from_pack(&mut pack, |n| n.starts_with("minecraft:block/")).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["minecraft:block/stone", "minecraft:block/water_still"]
        );
        let water = &entries[1];
        assert_eq!((water.width, water.height), (16, 16));
        assert_eq!(&water.rgba[..4], &[1, 2, 3, 255]);
        assert_eq!(water.flags, 0, "flags describe the first frame only");
    }

    #[test]
    fn equal_areas_prefer_the_squarer_shape() {
        let entries: Vec<_> = (0..1000_u32)
            .map(|i| solid(&format!("t{i}"), 16, 16, [1, 2, 3, 255]))
            .collect();
        let atlas = Atlas::build(entries, 1, 4096).unwrap();
        let stretch = atlas.width.max(atlas.height) / atlas.width.min(atlas.height);
        assert!(stretch <= 2, "{}x{}", atlas.width, atlas.height);
    }
}
