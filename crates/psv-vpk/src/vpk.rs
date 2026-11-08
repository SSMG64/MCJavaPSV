use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Seek, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::sfo::ParamSfo;

pub const EBOOT_PATH: &str = "eboot.bin";
pub const SFO_PATH: &str = "sce_sys/param.sfo";

const STORED_EXTENSIONS: &[&str] = &["gxt", "jpg", "lz4", "ogg", "pak", "png", "zip", "zst"];
const FORBIDDEN_CHARS: &[char] = &['\\', ':', '*', '?', '"', '<', '>', '|'];

#[derive(Debug, Error)]
pub enum VpkError {
    #[error("invalid package path {path:?}: {reason}")]
    InvalidPath { path: String, reason: &'static str },
    #[error("{0:?} is already in the package, or differs from an existing entry only by case")]
    Duplicate(String),
    #[error("could not read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("could not write the package: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("could not write the package: {0}")]
    Io(#[from] io::Error),
    #[error("could not encode an image: {0}")]
    Image(#[from] png::EncodingError),
}

#[derive(Debug)]
enum Source {
    Memory(Vec<u8>),
    File(PathBuf),
}

#[derive(Debug)]
struct Entry {
    path: String,
    source: Source,
}

/// Collects the files of a Vita package and writes them as a deterministic ZIP.
#[derive(Debug)]
pub struct VpkBuilder {
    entries: Vec<Entry>,
    seen: BTreeSet<String>,
}

impl VpkBuilder {
    pub fn new(eboot: Vec<u8>, sfo: &ParamSfo) -> Self {
        let mut builder = Self {
            entries: Vec::new(),
            seen: BTreeSet::new(),
        };
        builder.push(SFO_PATH, Source::Memory(sfo.to_bytes()));
        builder.push(EBOOT_PATH, Source::Memory(eboot));
        builder
    }

    pub fn add_bytes(&mut self, path: &str, data: Vec<u8>) -> Result<&mut Self, VpkError> {
        self.reserve(path)?;
        self.push(path, Source::Memory(data));
        Ok(self)
    }

    pub fn add_file(
        &mut self,
        path: &str,
        file: impl Into<PathBuf>,
    ) -> Result<&mut Self, VpkError> {
        self.reserve(path)?;
        self.push(path, Source::File(file.into()));
        Ok(self)
    }

    /// Adds every regular file below `root` under `prefix`, in sorted order.
    pub fn add_dir(&mut self, root: &Path, prefix: &str) -> Result<&mut Self, VpkError> {
        let mut files = Vec::new();
        collect_files(root, &mut files).map_err(|source| VpkError::Read {
            path: root.to_path_buf(),
            source,
        })?;
        files.sort();
        for file in files {
            let relative = file.strip_prefix(root).map_err(|_| VpkError::InvalidPath {
                path: file.display().to_string(),
                reason: "outside the source directory",
            })?;
            let mut name = prefix.trim_end_matches('/').to_owned();
            for part in relative.components() {
                let part = part
                    .as_os_str()
                    .to_str()
                    .ok_or_else(|| VpkError::InvalidPath {
                        path: file.display().to_string(),
                        reason: "file name is not valid UTF-8",
                    })?;
                if !name.is_empty() {
                    name.push('/');
                }
                name.push_str(part);
            }
            self.add_file(&name, file)?;
        }
        Ok(self)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn write_to_path(&self, path: &Path) -> Result<(), VpkError> {
        let file = File::create(path)?;
        self.write_to(io::BufWriter::new(file))
    }

    pub fn write_to<W: Write + Seek>(&self, sink: W) -> Result<(), VpkError> {
        let mut zip = ZipWriter::new(sink);
        for entry in &self.entries {
            match &entry.source {
                Source::Memory(data) => {
                    zip.start_file(&entry.path, options_for(&entry.path, data.len() as u64))?;
                    zip.write_all(data)?;
                }
                Source::File(path) => {
                    let read_error = |source| VpkError::Read {
                        path: path.clone(),
                        source,
                    };
                    let mut file = File::open(path).map_err(read_error)?;
                    let len = file.metadata().map_err(read_error)?.len();
                    zip.start_file(&entry.path, options_for(&entry.path, len))?;
                    io::copy(&mut file, &mut zip).map_err(read_error)?;
                }
            }
        }
        zip.finish()?;
        Ok(())
    }

    fn reserve(&mut self, path: &str) -> Result<(), VpkError> {
        validate_path(path)?;
        if self.seen.contains(&path.to_ascii_lowercase()) {
            return Err(VpkError::Duplicate(path.to_owned()));
        }
        Ok(())
    }

    fn push(&mut self, path: &str, source: Source) {
        self.seen.insert(path.to_ascii_lowercase());
        self.entries.push(Entry {
            path: path.to_owned(),
            source,
        });
    }
}

fn options_for(path: &str, len: u64) -> SimpleFileOptions {
    let stored = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| STORED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()));
    SimpleFileOptions::default()
        .compression_method(if stored {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        })
        .last_modified_time(DateTime::default())
        .large_file(len > u64::from(u32::MAX))
}

fn validate_path(path: &str) -> Result<(), VpkError> {
    let invalid = |reason| VpkError::InvalidPath {
        path: path.to_owned(),
        reason,
    };
    if path.is_empty() {
        return Err(invalid("empty"));
    }
    if path.starts_with('/') || path.ends_with('/') {
        return Err(invalid("must be relative and name a file"));
    }
    if path.contains(FORBIDDEN_CHARS) || path.chars().any(char::is_control) {
        return Err(invalid("contains a character FAT and Windows cannot store"));
    }
    for part in path.split('/') {
        match part {
            "" => return Err(invalid("contains an empty component")),
            "." | ".." => return Err(invalid("contains a dot component")),
            _ if part.ends_with(['.', ' ']) => {
                return Err(invalid("a component ends with a dot or a space"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect_files(&entry.path(), out)?;
        } else if kind.is_file() {
            out.push(entry.path());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use zip::ZipArchive;

    use super::*;

    fn sfo() -> ParamSfo {
        ParamSfo::homebrew("MCJVPSV01", "MCJavaPSV").unwrap()
    }

    fn built(builder: &VpkBuilder) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        builder.write_to(&mut out).unwrap();
        out.into_inner()
    }

    #[test]
    fn contains_required_entries_first() {
        let builder = VpkBuilder::new(b"ELF".to_vec(), &sfo());
        let mut archive = ZipArchive::new(Cursor::new(built(&builder))).unwrap();
        assert_eq!(archive.len(), 2);
        assert_eq!(archive.by_index(0).unwrap().name().unwrap(), SFO_PATH);
        let mut eboot = String::new();
        archive
            .by_name(EBOOT_PATH)
            .unwrap()
            .read_to_string(&mut eboot)
            .unwrap();
        assert_eq!(eboot, "ELF");
    }

    #[test]
    fn output_is_deterministic() {
        let make = || {
            let mut b = VpkBuilder::new(vec![1, 2, 3], &sfo());
            b.add_bytes("data/a.bin", vec![9; 100]).unwrap();
            built(&b)
        };
        assert_eq!(make(), make());
    }

    #[test]
    fn already_compressed_formats_are_stored() {
        let mut b = VpkBuilder::new(vec![], &sfo());
        b.add_bytes("snd/a.ogg", vec![0; 64]).unwrap();
        b.add_bytes("data/a.bin", vec![0; 64]).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(built(&b))).unwrap();
        assert_eq!(
            archive.by_name("snd/a.ogg").unwrap().compression(),
            CompressionMethod::Stored
        );
        assert_eq!(
            archive.by_name("data/a.bin").unwrap().compression(),
            CompressionMethod::Deflated
        );
    }

    #[test]
    fn rejects_unsafe_paths() {
        let mut b = VpkBuilder::new(vec![], &sfo());
        for path in [
            "",
            "/abs",
            "dir/",
            "a/../b",
            "a//b",
            "./a",
            "a\\b",
            "c:x",
            "bad?.bin",
            "trailing.",
            "sp ace /x",
        ] {
            assert!(
                matches!(b.add_bytes(path, vec![]), Err(VpkError::InvalidPath { .. })),
                "{path:?} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_duplicates_ignoring_case_and_reserved_names() {
        let mut b = VpkBuilder::new(vec![], &sfo());
        b.add_bytes("Data/x.bin", vec![]).unwrap();
        assert!(matches!(
            b.add_bytes("data/X.BIN", vec![]),
            Err(VpkError::Duplicate(_))
        ));
        assert!(matches!(
            b.add_bytes("EBOOT.BIN", vec![]),
            Err(VpkError::Duplicate(_))
        ));
        assert!(matches!(
            b.add_bytes("sce_sys/param.sfo", vec![]),
            Err(VpkError::Duplicate(_))
        ));
    }

    #[test]
    fn add_dir_walks_sorted_and_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("b/inner")).unwrap();
        fs::write(dir.path().join("b/inner/z.txt"), "z").unwrap();
        fs::write(dir.path().join("a.txt"), "a").unwrap();

        let mut b = VpkBuilder::new(vec![], &sfo());
        b.add_dir(dir.path(), "assets/").unwrap();
        let mut archive = ZipArchive::new(Cursor::new(built(&b))).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().unwrap().into_owned())
            .collect();
        assert_eq!(
            names,
            [SFO_PATH, EBOOT_PATH, "assets/a.txt", "assets/b/inner/z.txt"]
        );
    }

    #[test]
    fn missing_source_file_is_a_read_error() {
        let mut b = VpkBuilder::new(vec![], &sfo());
        b.add_file("x.bin", "/definitely/not/here").unwrap();
        let mut out = Cursor::new(Vec::new());
        assert!(matches!(b.write_to(&mut out), Err(VpkError::Read { .. })));
    }
}
