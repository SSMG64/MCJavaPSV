use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Where the baker reads files from. Paths use `/` and are relative to the source's root.
pub trait Source {
    fn list(&mut self) -> io::Result<Vec<String>>;
    fn read(&mut self, path: &str) -> io::Result<Vec<u8>>;
}

pub struct TreeSource {
    root: PathBuf,
}

impl TreeSource {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }
}

fn collect(dir: &Path, prefix: &str, out: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(&entry.path(), &relative, out)?;
        } else if kind.is_file() {
            out.push(relative);
        }
    }
    Ok(())
}

impl Source for TreeSource {
    fn list(&mut self) -> io::Result<Vec<String>> {
        let mut paths = Vec::new();
        collect(&self.root, "", &mut paths)?;
        paths.sort();
        Ok(paths)
    }

    fn read(&mut self, path: &str) -> io::Result<Vec<u8>> {
        fs::read(self.root.join(path))
    }
}

pub struct ZipSource {
    archive: zip::ZipArchive<File>,
    prefix: String,
    names: Vec<(String, usize)>,
}

impl ZipSource {
    /// `prefix` is removed from every name, and entries outside it are ignored.
    pub fn open(path: &Path, prefix: Option<&str>) -> Result<Self, zip::result::ZipError> {
        let mut archive = zip::ZipArchive::new(File::open(path)?)?;
        let prefix = prefix.unwrap_or("").to_owned();
        let mut names = Vec::new();
        for index in 0..archive.len() {
            let entry = archive.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name()?.into_owned();
            if let Some(stripped) = name.strip_prefix(&prefix) {
                names.push((stripped.to_owned(), index));
            }
        }
        names.sort();
        Ok(Self {
            archive,
            prefix,
            names,
        })
    }
}

impl Source for ZipSource {
    fn list(&mut self) -> io::Result<Vec<String>> {
        Ok(self.names.iter().map(|(name, _)| name.clone()).collect())
    }

    fn read(&mut self, path: &str) -> io::Result<Vec<u8>> {
        let (_, index) = self
            .names
            .binary_search_by(|(name, _)| name.as_str().cmp(path))
            .map(|at| &self.names[at])
            .map_err(|_| {
                io::Error::new(io::ErrorKind::NotFound, format!("{}{path}", self.prefix))
            })?;
        let mut entry = self.archive.by_index(*index).map_err(io::Error::other)?;
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}
