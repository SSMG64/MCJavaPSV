//! The bake stage: a prepared resource folder in, one Vita data pack out.
//!
//! Every PNG becomes a lossless [`psv_pack::texture`] blob, files the Vita cannot use are
//! skipped, and everything else is stored or LZ4 compressed. Nothing is ever lossy: a baked
//! texture decodes to exactly the pixels of its PNG.

pub mod classify;
pub mod source;

use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use psv_pack::{CodecChoice, PackError, PackStats, PackWriter, texture};
use thiserror::Error;

pub use classify::{Action, SkipReason, classify};
pub use source::{Source, TreeSource, ZipSource};

const MAX_WARNINGS: usize = 100;

#[derive(Debug, Error)]
pub enum BakeError {
    #[error(transparent)]
    Pack(#[from] PackError),
    #[error("could not read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("could not read the zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("could not write {path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
    #[error("cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BakeStats {
    pub files: u32,
    pub textures: u32,
    pub indexed_textures: u32,
    pub raw_textures: u32,
    pub png_fallbacks: u32,
    pub skipped: u32,
    pub invalid_paths: u32,
    pub png_bytes: u64,
    pub texture_bytes: u64,
    pub source_bytes: u64,
    pub warnings: Vec<String>,
    pub pack: PackStats,
}

impl BakeStats {
    fn warn(&mut self, message: String) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(message);
        }
    }
}

fn read(source: &mut impl Source, path: &str) -> Result<Vec<u8>, BakeError> {
    source.read(path).map_err(|error| BakeError::Read {
        path: path.to_owned(),
        source: error,
    })
}

/// Bakes every file of `source` into `pack`. `progress` receives `(done, total)`.
pub fn bake<S: Source, W: Write + Seek>(
    source: &mut S,
    pack: &mut PackWriter<W>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<BakeStats, BakeError> {
    let mut stats = BakeStats::default();
    let paths = source.list().map_err(|error| BakeError::Read {
        path: "<listing>".to_owned(),
        source: error,
    })?;
    let total = paths.len();

    for (done, path) in paths.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(BakeError::Cancelled);
        }
        progress(done, total);

        match classify(path) {
            Action::Skip(_) => stats.skipped += 1,
            Action::Texture(target) => {
                let bytes = read(source, path)?;
                stats.source_bytes += bytes.len() as u64;
                stats.png_bytes += bytes.len() as u64;
                let baked = psv_vpk::icon::decode_png(&bytes).ok().and_then(|image| {
                    texture::encode(image.width, image.height, &image.pixels).ok()
                });
                match baked {
                    Some(blob) => {
                        stats.texture_bytes += blob.len() as u64;
                        stats.textures += 1;
                        if blob[5] == 1 {
                            stats.indexed_textures += 1;
                        } else {
                            stats.raw_textures += 1;
                        }
                        add(pack, &target, &blob, CodecChoice::Auto, &mut stats)?;
                    }
                    None => {
                        stats.png_fallbacks += 1;
                        stats.warn(format!("{path}: not a decodable PNG, kept as is"));
                        add(pack, path, &bytes, CodecChoice::Store, &mut stats)?;
                    }
                }
            }
            Action::File(choice) => {
                let bytes = read(source, path)?;
                stats.source_bytes += bytes.len() as u64;
                stats.files += 1;
                add(pack, path, &bytes, choice, &mut stats)?;
            }
        }
    }
    progress(total, total);
    Ok(stats)
}

fn add<W: Write + Seek>(
    pack: &mut PackWriter<W>,
    path: &str,
    data: &[u8],
    choice: CodecChoice,
    stats: &mut BakeStats,
) -> Result<(), BakeError> {
    match pack.add(path, data, choice) {
        Ok(_) => Ok(()),
        Err(PackError::InvalidPath { reason, .. }) => {
            stats.invalid_paths += 1;
            stats.warn(format!("{path}: skipped, {reason}"));
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

/// Bakes into a new file at `output`, replacing it only when the whole bake succeeded.
pub fn bake_to_file<S: Source>(
    source: &mut S,
    output: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<BakeStats, BakeError> {
    let write_error = |error| BakeError::Write {
        path: output.display().to_string(),
        source: error,
    };
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(write_error)?;
    }
    let partial = output.with_extension("pak.part");
    let file = File::create(&partial).map_err(write_error)?;
    let mut pack = PackWriter::new(BufWriter::new(file))?;

    let result = bake(source, &mut pack, cancel, progress).and_then(|mut stats| {
        let (mut sink, pack_stats) = pack.finish()?;
        sink.flush().map_err(write_error)?;
        stats.pack = pack_stats;
        Ok(stats)
    });
    match result {
        Ok(stats) => {
            std::fs::rename(&partial, output).map_err(write_error)?;
            Ok(stats)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&partial);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use psv_pack::PackReader;

    use super::*;

    fn png_bytes(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(rgba)
            .unwrap();
        out
    }

    fn checker(width: u32, height: u32) -> Vec<u8> {
        (0..width * height)
            .flat_map(|i| {
                if i % 2 == 0 {
                    [200, 30, 30, 255]
                } else {
                    [10, 10, 10, 0]
                }
            })
            .collect()
    }

    fn write(root: &Path, path: &str, bytes: &[u8]) {
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, bytes).unwrap();
    }

    fn bake_tree(root: &Path) -> (PackReader<Cursor<Vec<u8>>>, BakeStats) {
        let mut pack = PackWriter::new(Cursor::new(Vec::new())).unwrap();
        let mut source = TreeSource::new(root);
        let mut stats = bake(
            &mut source,
            &mut pack,
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap();
        let (sink, pack_stats) = pack.finish().unwrap();
        stats.pack = pack_stats;
        (
            PackReader::open(Cursor::new(sink.into_inner())).unwrap(),
            stats,
        )
    }

    #[test]
    fn bakes_textures_losslessly_and_keeps_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let pixels = checker(16, 16);
        write(
            dir.path(),
            "assets/minecraft/textures/block/a.png",
            &png_bytes(16, 16, &pixels),
        );
        write(
            dir.path(),
            "assets/minecraft/textures/block/a.png.mcmeta",
            b"{\"animation\":{}}",
        );
        write(
            dir.path(),
            "assets/minecraft/models/block/a.json",
            br#"{"parent":"x"}"#,
        );
        write(
            dir.path(),
            "assets/minecraft/sounds/x.ogg",
            b"OggS-fake-audio",
        );
        write(
            dir.path(),
            "assets/minecraft/shaders/core/a.fsh",
            b"void main(){}",
        );
        write(dir.path(), "META-INF/MANIFEST.MF", b"x");

        let (mut pack, stats) = bake_tree(dir.path());
        let texture =
            texture::decode(&pack.read("assets/minecraft/textures/block/a.tex").unwrap()).unwrap();
        assert_eq!(texture.rgba, pixels);
        assert!(!pack.contains("assets/minecraft/textures/block/a.png"));
        assert_eq!(
            pack.read("assets/minecraft/textures/block/a.png.mcmeta")
                .unwrap(),
            b"{\"animation\":{}}"
        );
        assert_eq!(
            pack.read("assets/minecraft/models/block/a.json").unwrap(),
            br#"{"parent":"x"}"#
        );
        assert_eq!(
            pack.read("assets/minecraft/sounds/x.ogg").unwrap(),
            b"OggS-fake-audio"
        );
        assert!(!pack.contains("assets/minecraft/shaders/core/a.fsh"));
        assert!(!pack.contains("META-INF/MANIFEST.MF"));

        assert_eq!(
            (
                stats.textures,
                stats.indexed_textures,
                stats.files,
                stats.skipped
            ),
            (1, 1, 3, 2)
        );
        assert_eq!(stats.png_fallbacks, 0);
    }

    #[test]
    fn undecodable_png_is_kept_as_is_with_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "assets/minecraft/textures/bad.png",
            b"not really a png",
        );
        let (mut pack, stats) = bake_tree(dir.path());
        assert_eq!(
            pack.read("assets/minecraft/textures/bad.png").unwrap(),
            b"not really a png"
        );
        assert_eq!(stats.png_fallbacks, 1);
        assert!(stats.warnings[0].contains("bad.png"));
    }

    #[test]
    fn invalid_pack_paths_are_skipped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "ok/file.txt", b"fine");
        write(dir.path(), "odd/what?.txt", b"bad name");
        let (pack, stats) = bake_tree(dir.path());
        if cfg!(windows) {
            return;
        }
        assert!(pack.contains("ok/file.txt"));
        assert!(!pack.contains("odd/what?.txt"));
        assert_eq!(stats.invalid_paths, 1);
    }

    #[test]
    fn output_is_deterministic_and_reports_progress() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            write(
                dir.path(),
                &format!("d/{i}.json"),
                format!("{{\"n\":{i}}}").as_bytes(),
            );
        }
        let run = || {
            let mut calls = Vec::new();
            let mut pack = PackWriter::new(Cursor::new(Vec::new())).unwrap();
            bake(
                &mut TreeSource::new(dir.path()),
                &mut pack,
                &AtomicBool::new(false),
                &mut |done, total| calls.push((done, total)),
            )
            .unwrap();
            (pack.finish().unwrap().0.into_inner(), calls)
        };
        let (first, calls) = run();
        assert_eq!(first, run().0);
        assert_eq!(calls.first(), Some(&(0, 5)));
        assert_eq!(calls.last(), Some(&(5, 5)));
    }

    #[test]
    fn cancel_stops_the_bake() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.json", b"{}");
        let mut pack = PackWriter::new(Cursor::new(Vec::new())).unwrap();
        let result = bake(
            &mut TreeSource::new(dir.path()),
            &mut pack,
            &AtomicBool::new(true),
            &mut |_, _| {},
        );
        assert!(matches!(result, Err(BakeError::Cancelled)));
    }

    #[test]
    fn failed_bake_leaves_no_output_file() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "in/a.json", b"{}");
        let output = dir.path().join("out/data.pak");
        let result = bake_to_file(
            &mut TreeSource::new(&dir.path().join("in")),
            &output,
            &AtomicBool::new(true),
            &mut |_, _| {},
        );
        assert!(matches!(result, Err(BakeError::Cancelled)));
        assert!(!output.exists());
        assert!(!output.with_extension("pak.part").exists());
    }

    #[test]
    fn bakes_from_a_zip_with_a_prefix_to_strip() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("jar.zip");
        {
            let file = File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("root/assets/m/a.json", options).unwrap();
            zip.write_all(b"{\"a\":1}").unwrap();
            zip.start_file("root/assets/m/t.png", options).unwrap();
            zip.write_all(&png_bytes(2, 2, &checker(2, 2))).unwrap();
            zip.finish().unwrap();
        }
        let output = dir.path().join("o.pak");
        let stats = bake_to_file(
            &mut ZipSource::open(&zip_path, Some("root/")).unwrap(),
            &output,
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!((stats.files, stats.textures), (1, 1));
        let mut pack = PackReader::open(File::open(&output).unwrap()).unwrap();
        assert_eq!(pack.read("assets/m/a.json").unwrap(), b"{\"a\":1}");
        assert!(pack.contains("assets/m/t.tex"));
    }
}
