//! Original application icon, generated in code, plus the small image helpers the
//! builder needs to turn any square PNG into a Windows icon.

use std::io::Cursor;

use thiserror::Error;

const DEEP: [u8; 3] = [14, 38, 56];
const LIGHT: [u8; 3] = [46, 112, 138];
const ACCENT: [u8; 3] = [235, 190, 80];

pub const ICO_SIZES: [u32; 6] = [16, 32, 48, 64, 128, 256];
const ICO_MAX_SIZE: u32 = 256;

#[derive(Debug, Error)]
pub enum IconError {
    #[error("the image is {width}x{height}, but an icon must be square")]
    NotSquare { width: u32, height: u32 },
    #[error("icons can be at most {ICO_MAX_SIZE} pixels wide, got {0}")]
    TooLarge(u32),
    #[error("the image has no pixels")]
    Empty,
    #[error("could not read the PNG: {0}")]
    Decode(#[from] png::DecodingError),
    #[error("could not write the PNG: {0}")]
    Encode(#[from] png::EncodingError),
    #[error("unsupported PNG pixel layout")]
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    fn at(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }
}

fn scaled(size: u32, numerator: u32) -> u32 {
    (size * numerator + 64) / 128
}

/// The project's own icon at any size: a gradient with three nested squares.
pub fn generated(size: u32) -> Rgba {
    let size = size.max(1);
    let mut pixels = Vec::with_capacity(size as usize * size as usize * 4);
    for y in 0..size {
        let t = y as f32 / (size - 1).max(1) as f32;
        let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        let row = [
            mix(LIGHT[0], DEEP[0]),
            mix(LIGHT[1], DEEP[1]),
            mix(LIGHT[2], DEEP[2]),
            255,
        ];
        for _ in 0..size {
            pixels.extend_from_slice(&row);
        }
    }
    let mut image = Rgba {
        width: size,
        height: size,
        pixels,
    };
    let centre = size / 2;
    for (half, color) in [(44, ACCENT), (36, DEEP), (20, ACCENT)] {
        let half = scaled(size, half).max(1);
        for y in centre.saturating_sub(half)..(centre + half).min(size) {
            for x in centre.saturating_sub(half)..(centre + half).min(size) {
                let at = image.at(x, y);
                image.pixels[at..at + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
    }
    image
}

/// Resamples a square image to `size` by averaging the source pixels each target
/// pixel covers. Growing an image repeats pixels.
pub fn resize(source: &Rgba, size: u32) -> Result<Rgba, IconError> {
    if source.width == 0 || source.height == 0 {
        return Err(IconError::Empty);
    }
    if source.width != source.height {
        return Err(IconError::NotSquare {
            width: source.width,
            height: source.height,
        });
    }
    let size = size.max(1);
    let span = |target: u32| {
        let start = (u64::from(target) * u64::from(source.width) / u64::from(size)) as u32;
        let end =
            ((u64::from(target) + 1) * u64::from(source.width)).div_ceil(u64::from(size)) as u32;
        (start, end.clamp(start + 1, source.width))
    };

    let mut pixels = Vec::with_capacity(size as usize * size as usize * 4);
    for ty in 0..size {
        let (y0, y1) = span(ty);
        for tx in 0..size {
            let (x0, x1) = span(tx);
            let mut sum = [0_u64; 4];
            let mut count = 0_u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let at = source.at(x, y);
                    for (total, &value) in sum.iter_mut().zip(&source.pixels[at..at + 4]) {
                        *total += u64::from(value);
                    }
                    count += 1;
                }
            }
            pixels.extend(sum.iter().map(|&s| ((s + count / 2) / count) as u8));
        }
    }
    Ok(Rgba {
        width: size,
        height: size,
        pixels,
    })
}

pub fn encode_png(image: &Rgba) -> Result<Vec<u8>, IconError> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&image.pixels)?;
    Ok(out)
}

pub fn decode_png(bytes: &[u8]) -> Result<Rgba, IconError> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or(IconError::Unsupported)?];
    let frame = reader.next_frame(&mut buffer)?;
    let data = &buffer[..frame.buffer_size()];

    let pixels = match frame.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => data
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => return Err(IconError::Unsupported),
    };
    Ok(Rgba {
        width: frame.width,
        height: frame.height,
        pixels,
    })
}

/// Packs square images of at most 256 pixels into a Windows `.ico` with PNG entries.
pub fn ico(images: &[Rgba]) -> Result<Vec<u8>, IconError> {
    let mut blobs = Vec::with_capacity(images.len());
    for image in images {
        if image.width != image.height {
            return Err(IconError::NotSquare {
                width: image.width,
                height: image.height,
            });
        }
        if image.width > ICO_MAX_SIZE {
            return Err(IconError::TooLarge(image.width));
        }
        blobs.push(encode_png(image)?);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (image, blob) in images.iter().zip(&blobs) {
        let side = if image.width == ICO_MAX_SIZE {
            0
        } else {
            image.width as u8
        };
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&32_u16.to_le_bytes());
        out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += blob.len() as u32;
    }
    for blob in blobs {
        out.extend_from_slice(&blob);
    }
    Ok(out)
}

/// The standard set of icon sizes, resampled from one master image.
pub fn ico_from_master(master: &Rgba) -> Result<Vec<u8>, IconError> {
    let images = ICO_SIZES
        .iter()
        .map(|&size| resize(master, size))
        .collect::<Result<Vec<_>, _>>()?;
    ico(&images)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(size: u32, color: [u8; 4]) -> Rgba {
        Rgba {
            width: size,
            height: size,
            pixels: color.repeat(size as usize * size as usize),
        }
    }

    #[test]
    fn generated_icon_has_the_requested_size_and_is_deterministic() {
        for size in [16, 48, 256] {
            let image = generated(size);
            assert_eq!((image.width, image.height), (size, size));
            assert_eq!(image.pixels.len(), size as usize * size as usize * 4);
            assert_eq!(image, generated(size));
        }
    }

    #[test]
    fn generated_icon_has_a_visible_centre_mark() {
        let image = generated(128);
        let centre = image.at(64, 64);
        assert_eq!(&image.pixels[centre..centre + 4], &[235, 190, 80, 255]);
    }

    #[test]
    fn png_round_trips() {
        let image = generated(32);
        assert_eq!(decode_png(&encode_png(&image).unwrap()).unwrap(), image);
    }

    #[test]
    fn decoding_expands_rgb_and_grey() {
        let mut rgb = Vec::new();
        let mut encoder = png::Encoder::new(&mut rgb, 2, 2);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[1, 2, 3].repeat(4))
            .unwrap();
        let decoded = decode_png(&rgb).unwrap();
        assert_eq!(&decoded.pixels[..4], &[1, 2, 3, 255]);

        let mut grey = Vec::new();
        let mut encoder = png::Encoder::new(&mut grey, 1, 1);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[9])
            .unwrap();
        assert_eq!(decode_png(&grey).unwrap().pixels, [9, 9, 9, 255]);
    }

    #[test]
    fn garbage_is_not_a_png() {
        assert!(matches!(decode_png(b"nope"), Err(IconError::Decode(_))));
    }

    #[test]
    fn resize_keeps_solid_colours_and_averages_blocks() {
        let flat = resize(&solid(64, [10, 20, 30, 255]), 16).unwrap();
        assert!(
            flat.pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [10, 20, 30, 255])
        );

        let mut checker = solid(2, [0, 0, 0, 255]);
        checker.pixels[0..4].copy_from_slice(&[200, 200, 200, 255]);
        let one = resize(&checker, 1).unwrap();
        assert_eq!(&one.pixels[..4], &[50, 50, 50, 255]);
    }

    #[test]
    fn resize_can_grow_and_rejects_bad_input() {
        let big = resize(&solid(2, [1, 2, 3, 4]), 8).unwrap();
        assert_eq!((big.width, big.pixels.len()), (8, 8 * 8 * 4));
        let wide = Rgba {
            width: 2,
            height: 1,
            pixels: vec![0; 8],
        };
        assert!(matches!(resize(&wide, 4), Err(IconError::NotSquare { .. })));
        let empty = Rgba {
            width: 0,
            height: 0,
            pixels: vec![],
        };
        assert!(matches!(resize(&empty, 4), Err(IconError::Empty)));
    }

    #[test]
    fn ico_directory_points_at_valid_pngs() {
        let bytes = ico_from_master(&generated(256)).unwrap();
        assert_eq!(&bytes[0..4], &[0, 0, 1, 0]);
        let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
        assert_eq!(count, ICO_SIZES.len());
        for (i, &expected) in ICO_SIZES.iter().enumerate() {
            let entry = &bytes[6 + 16 * i..22 + 16 * i];
            let side = if entry[0] == 0 {
                256
            } else {
                u32::from(entry[0])
            };
            assert_eq!(side, expected);
            let len = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
            let at = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
            let image = decode_png(&bytes[at..at + len]).unwrap();
            assert_eq!(image.width, expected);
        }
    }

    #[test]
    fn ico_rejects_oversized_and_non_square_images() {
        assert!(matches!(
            ico(&[solid(257, [0; 4])]),
            Err(IconError::TooLarge(257))
        ));
        let wide = Rgba {
            width: 2,
            height: 1,
            pixels: vec![0; 8],
        };
        assert!(matches!(ico(&[wide]), Err(IconError::NotSquare { .. })));
    }
}
