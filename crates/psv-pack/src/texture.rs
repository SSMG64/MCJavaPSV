//! A lossless texture: the PNG's pixels without the inflate step.
//!
//! Layout, little endian: `MCTX`, version, kind, flags, one reserved byte, width, height,
//! then either raw RGBA rows or a palette of up to 256 RGBA colors followed by one index byte
//! per pixel. The encoder picks whichever is smaller. Neither form loses anything.

use std::collections::HashMap;

use thiserror::Error;

pub const MAGIC: [u8; 4] = *b"MCTX";
pub const VERSION: u8 = 1;
const HEADER_LEN: usize = 16;
const KIND_RAW: u8 = 0;
const KIND_INDEXED: u8 = 1;
const MAX_PALETTE: usize = 256;
pub const MAX_SIDE: u32 = 16_384;

/// At least one pixel is fully transparent.
pub const FLAG_HAS_TRANSPARENT: u8 = 1;
/// At least one pixel is partly transparent, so the texture needs blending.
pub const FLAG_HAS_TRANSLUCENT: u8 = 2;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TextureError {
    #[error("{0}x{1} is not a size a texture can have")]
    BadSize(u32, u32),
    #[error("expected {expected} bytes of pixels, got {actual}")]
    WrongPixelCount { expected: usize, actual: usize },
    #[error("not a texture: {0}")]
    Corrupt(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub flags: u8,
    pub rgba: Vec<u8>,
}

fn pixel_bytes(width: u32, height: u32) -> Result<usize, TextureError> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(TextureError::BadSize(width, height));
    }
    Ok(width as usize * height as usize * 4)
}

pub fn flags_of(rgba: &[u8]) -> u8 {
    let mut flags = 0;
    for pixel in rgba.as_chunks::<4>().0 {
        match pixel[3] {
            0 => flags |= FLAG_HAS_TRANSPARENT,
            255 => {}
            _ => flags |= FLAG_HAS_TRANSLUCENT,
        }
    }
    flags
}

fn palette_of(rgba: &[u8]) -> Option<(Vec<[u8; 4]>, Vec<u8>)> {
    let mut lookup: HashMap<[u8; 4], u8> = HashMap::new();
    let mut palette = Vec::new();
    let mut indices = Vec::with_capacity(rgba.len() / 4);
    for &pixel in rgba.as_chunks::<4>().0 {
        let index = match lookup.get(&pixel) {
            Some(&index) => index,
            None => {
                if palette.len() == MAX_PALETTE {
                    return None;
                }
                let index = palette.len() as u8;
                lookup.insert(pixel, index);
                palette.push(pixel);
                index
            }
        };
        indices.push(index);
    }
    Some((palette, indices))
}

pub fn encode(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, TextureError> {
    let expected = pixel_bytes(width, height)?;
    if rgba.len() != expected {
        return Err(TextureError::WrongPixelCount {
            expected,
            actual: rgba.len(),
        });
    }

    let mut out = Vec::with_capacity(HEADER_LEN + expected);
    out.extend_from_slice(&MAGIC);
    out.push(VERSION);
    let kind_at = out.len();
    out.push(KIND_RAW);
    out.push(flags_of(rgba));
    out.push(0);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());

    match palette_of(rgba) {
        Some((palette, indices)) if 2 + palette.len() * 4 + indices.len() < expected => {
            out[kind_at] = KIND_INDEXED;
            out.extend_from_slice(&(palette.len() as u16).to_le_bytes());
            for color in &palette {
                out.extend_from_slice(color);
            }
            out.extend_from_slice(&indices);
        }
        _ => out.extend_from_slice(rgba),
    }
    Ok(out)
}

pub fn decode(bytes: &[u8]) -> Result<Texture, TextureError> {
    if bytes.len() < HEADER_LEN {
        return Err(TextureError::Corrupt("shorter than a header"));
    }
    if bytes[0..4] != MAGIC {
        return Err(TextureError::Corrupt("bad magic"));
    }
    if bytes[4] != VERSION {
        return Err(TextureError::Corrupt("unsupported version"));
    }
    let (kind, flags) = (bytes[5], bytes[6]);
    let width = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let height = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let expected = pixel_bytes(width, height)?;
    let body = &bytes[HEADER_LEN..];

    let rgba = match kind {
        KIND_RAW => {
            if body.len() != expected {
                return Err(TextureError::Corrupt("the pixel data has the wrong length"));
            }
            body.to_vec()
        }
        KIND_INDEXED => {
            let count_bytes = body
                .get(..2)
                .ok_or(TextureError::Corrupt("the palette is missing"))?;
            let count = usize::from(u16::from_le_bytes([count_bytes[0], count_bytes[1]]));
            if count == 0 || count > MAX_PALETTE {
                return Err(TextureError::Corrupt("the palette size is out of range"));
            }
            let palette_end = 2 + count * 4;
            let palette = body
                .get(2..palette_end)
                .ok_or(TextureError::Corrupt("the palette is truncated"))?;
            let indices = &body[palette_end..];
            if indices.len() != expected / 4 {
                return Err(TextureError::Corrupt("the index data has the wrong length"));
            }
            let mut rgba = Vec::with_capacity(expected);
            for &index in indices {
                let at = usize::from(index) * 4;
                let color = palette
                    .get(at..at + 4)
                    .ok_or(TextureError::Corrupt("a pixel points outside the palette"))?;
                rgba.extend_from_slice(color);
            }
            rgba
        }
        _ => return Err(TextureError::Corrupt("unknown texture kind")),
    };
    Ok(Texture {
        width,
        height,
        flags,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn few_colors(width: u32, height: u32) -> Vec<u8> {
        let colors = [[10, 20, 30, 255], [200, 100, 50, 255], [0, 0, 0, 0]];
        (0..width * height)
            .flat_map(|i| colors[(i % 3) as usize])
            .collect()
    }

    fn many_colors(width: u32, height: u32) -> Vec<u8> {
        (0..width * height)
            .flat_map(|i| {
                [
                    (i % 251) as u8,
                    (i / 7 % 256) as u8,
                    (i / 3 % 256) as u8,
                    255,
                ]
            })
            .collect()
    }

    #[test]
    fn few_colors_use_a_palette_and_round_trip() {
        let rgba = few_colors(16, 16);
        let bytes = encode(16, 16, &rgba).unwrap();
        assert_eq!(bytes[5], KIND_INDEXED);
        assert!(bytes.len() < rgba.len() / 2);
        let texture = decode(&bytes).unwrap();
        assert_eq!((texture.width, texture.height), (16, 16));
        assert_eq!(texture.rgba, rgba);
    }

    #[test]
    fn many_colors_stay_raw_and_round_trip() {
        let rgba = many_colors(64, 64);
        let bytes = encode(64, 64, &rgba).unwrap();
        assert_eq!(bytes[5], KIND_RAW);
        assert_eq!(decode(&bytes).unwrap().rgba, rgba);
    }

    #[test]
    fn exactly_256_colors_still_use_a_palette() {
        let rgba: Vec<u8> = (0..32 * 32_u32)
            .flat_map(|i| [(i % 256) as u8, 1, 2, 255])
            .collect();
        let bytes = encode(32, 32, &rgba).unwrap();
        assert_eq!(bytes[5], KIND_INDEXED);
        assert_eq!(decode(&bytes).unwrap().rgba, rgba);
    }

    #[test]
    fn tiny_images_that_would_grow_stay_raw() {
        let rgba = [1, 2, 3, 4];
        let bytes = encode(1, 1, &rgba).unwrap();
        assert_eq!(bytes[5], KIND_RAW);
        assert_eq!(decode(&bytes).unwrap().rgba, rgba);
    }

    #[test]
    fn flags_describe_transparency() {
        assert_eq!(flags_of(&[1, 2, 3, 255]), 0);
        assert_eq!(flags_of(&[1, 2, 3, 0]), FLAG_HAS_TRANSPARENT);
        assert_eq!(flags_of(&[1, 2, 3, 128]), FLAG_HAS_TRANSLUCENT);
        assert_eq!(
            flags_of(&[1, 2, 3, 0, 1, 2, 3, 128]),
            FLAG_HAS_TRANSPARENT | FLAG_HAS_TRANSLUCENT
        );
        let bytes = encode(3, 1, &few_colors(3, 1)).unwrap();
        assert_eq!(decode(&bytes).unwrap().flags, FLAG_HAS_TRANSPARENT);
    }

    #[test]
    fn invalid_sizes_and_lengths_are_rejected() {
        assert!(matches!(
            encode(0, 4, &[]),
            Err(TextureError::BadSize(0, 4))
        ));
        assert!(matches!(
            encode(MAX_SIDE + 1, 1, &[]),
            Err(TextureError::BadSize(..))
        ));
        assert!(matches!(
            encode(2, 2, &[0; 15]),
            Err(TextureError::WrongPixelCount { .. })
        ));
    }

    #[test]
    fn corrupt_input_never_panics() {
        let good = encode(16, 16, &few_colors(16, 16)).unwrap();
        for cut in 0..good.len() {
            assert!(decode(&good[..cut]).is_err(), "cut at {cut}");
        }
        let mut bad_magic = good.clone();
        bad_magic[0] = b'X';
        assert!(decode(&bad_magic).is_err());

        let mut bad_index = good.clone();
        let last = bad_index.len() - 1;
        bad_index[last] = 200;
        assert!(matches!(
            decode(&bad_index),
            Err(TextureError::Corrupt("a pixel points outside the palette"))
        ));

        let mut bad_kind = good.clone();
        bad_kind[5] = 9;
        assert!(decode(&bad_kind).is_err());

        let mut huge = good;
        huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(decode(&huge), Err(TextureError::BadSize(..))));
    }
}
