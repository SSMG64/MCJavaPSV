//! Original, generic LiveArea artwork. It is generated in code so the repository never
//! carries a game's logo or art.

use crate::vpk::{VpkBuilder, VpkError};

pub const ICON_PATH: &str = "sce_sys/icon0.png";
pub const TEMPLATE_PATH: &str = "sce_sys/livearea/contents/template.xml";
pub const BACKGROUND_PATH: &str = "sce_sys/livearea/contents/bg0.png";
pub const STARTUP_PATH: &str = "sce_sys/livearea/contents/startup.png";

pub const ICON_SIZE: (u32, u32) = (128, 128);
pub const BACKGROUND_SIZE: (u32, u32) = (840, 500);
pub const STARTUP_SIZE: (u32, u32) = (280, 158);

pub const TEMPLATE_XML: &str = r#"<?xml version="1.0" encoding="utf-8" standalone="yes"?>
<livearea style="a1" format-ver="01.00" content-rev="1">
  <livearea-background>
    <image>bg0.png</image>
  </livearea-background>
  <gate>
    <startup-image>startup.png</startup-image>
  </gate>
</livearea>
"#;

type Rgb = [u8; 3];

const DEEP: Rgb = [14, 38, 56];
const LIGHT: Rgb = [46, 112, 138];
const ACCENT: Rgb = [235, 190, 80];

struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn gradient(width: u32, height: u32, top: Rgb, bottom: Rgb) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            let t = y as f32 / (height - 1).max(1) as f32;
            let mix =
                |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
            let row = [
                mix(top[0], bottom[0]),
                mix(top[1], bottom[1]),
                mix(top[2], bottom[2]),
                255,
            ];
            for _ in 0..width {
                pixels.extend_from_slice(&row);
            }
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    fn rect(&mut self, x0: u32, y0: u32, x1: u32, y1: u32, color: Rgb) {
        for y in y0.min(self.height)..y1.min(self.height) {
            for x in x0.min(self.width)..x1.min(self.width) {
                let at = (y as usize * self.width as usize + x as usize) * 4;
                self.pixels[at..at + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
    }

    fn encode(&self) -> Result<Vec<u8>, png::EncodingError> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&self.pixels)?;
        Ok(out)
    }
}

fn nested_squares(canvas: &mut Canvas, rings: &[(u32, Rgb)]) {
    let (cx, cy) = (canvas.width / 2, canvas.height / 2);
    for &(half, color) in rings {
        canvas.rect(cx - half, cy - half, cx + half, cy + half, color);
    }
}

pub fn icon() -> Result<Vec<u8>, png::EncodingError> {
    let image = crate::icon::generated(ICON_SIZE.0);
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&image.pixels)?;
    Ok(out)
}

pub fn background() -> Result<Vec<u8>, png::EncodingError> {
    let mut canvas = Canvas::gradient(BACKGROUND_SIZE.0, BACKGROUND_SIZE.1, LIGHT, DEEP);
    canvas.rect(0, 470, BACKGROUND_SIZE.0, 500, ACCENT);
    canvas.encode()
}

pub fn startup() -> Result<Vec<u8>, png::EncodingError> {
    let mut canvas = Canvas::gradient(STARTUP_SIZE.0, STARTUP_SIZE.1, DEEP, LIGHT);
    nested_squares(&mut canvas, &[(40, ACCENT), (32, DEEP), (18, ACCENT)]);
    canvas.encode()
}

impl VpkBuilder {
    /// Adds an icon, a LiveArea page and its images, all generic and generated here.
    pub fn add_generic_livearea(&mut self) -> Result<&mut Self, VpkError> {
        self.add_bytes(ICON_PATH, icon()?)?;
        self.add_bytes(TEMPLATE_PATH, TEMPLATE_XML.as_bytes().to_vec())?;
        self.add_bytes(BACKGROUND_PATH, background()?)?;
        self.add_bytes(STARTUP_PATH, startup()?)?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use zip::ZipArchive;

    use super::*;
    use crate::sfo::ParamSfo;

    fn dimensions(bytes: &[u8]) -> (u32, u32) {
        let reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
        let info = reader.info();
        (info.width, info.height)
    }

    #[test]
    fn images_have_the_sizes_the_vita_expects() {
        assert_eq!(dimensions(&icon().unwrap()), ICON_SIZE);
        assert_eq!(dimensions(&background().unwrap()), BACKGROUND_SIZE);
        assert_eq!(dimensions(&startup().unwrap()), STARTUP_SIZE);
    }

    #[test]
    fn images_are_deterministic() {
        assert_eq!(icon().unwrap(), icon().unwrap());
    }

    #[test]
    fn template_references_files_that_are_packaged() {
        let sfo = ParamSfo::homebrew("MCJVPSV01", "t").unwrap();
        let mut builder = VpkBuilder::new(vec![], &sfo);
        builder.add_generic_livearea().unwrap();
        let mut out = Cursor::new(Vec::new());
        builder.write_to(&mut out).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(out.into_inner())).unwrap();

        let mut template = String::new();
        archive
            .by_name(TEMPLATE_PATH)
            .unwrap()
            .read_to_string(&mut template)
            .unwrap();
        let folder = "sce_sys/livearea/contents/";
        for reference in ["bg0.png", "startup.png"] {
            assert!(template.contains(reference));
            assert!(archive.by_name(&format!("{folder}{reference}")).is_ok());
        }
        assert!(archive.by_name(ICON_PATH).is_ok());
    }

    #[test]
    fn adding_twice_is_rejected_as_a_duplicate() {
        let sfo = ParamSfo::homebrew("MCJVPSV01", "t").unwrap();
        let mut builder = VpkBuilder::new(vec![], &sfo);
        builder.add_generic_livearea().unwrap();
        assert!(matches!(
            builder.add_generic_livearea(),
            Err(VpkError::Duplicate(_))
        ));
    }
}
