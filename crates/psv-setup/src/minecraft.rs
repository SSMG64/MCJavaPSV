use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha1::{Digest, Sha1};

use crate::error::SetupError;

pub const TARGET_VERSION: &str = "26.3";

#[derive(Debug, Clone)]
pub struct Install {
    pub dir: PathBuf,
    pub version_json: PathBuf,
    pub client_jar: PathBuf,
    pub client_sha1: Option<String>,
    pub server_sha1: Option<String>,
    pub java_major: Option<u32>,
    pub java_component: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VersionJson {
    #[serde(default)]
    downloads: Downloads,
    #[serde(default, rename = "javaVersion")]
    java_version: Option<JavaVersion>,
}

#[derive(Debug, Default, Deserialize)]
struct Downloads {
    client: Option<Artifact>,
    server: Option<Artifact>,
}

#[derive(Debug, Deserialize)]
struct Artifact {
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JavaVersion {
    component: Option<String>,
    #[serde(rename = "majorVersion")]
    major_version: Option<u32>,
}

pub fn default_minecraft_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join(".minecraft"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Library/Application Support/minecraft"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".minecraft"))
    }
}

impl Install {
    pub fn inspect(dir: &Path) -> Result<Self, SetupError> {
        Self::inspect_version(dir, TARGET_VERSION)
    }

    pub fn inspect_version(dir: &Path, version: &str) -> Result<Self, SetupError> {
        let folder = dir.join("versions").join(version);
        let version_json = folder.join(format!("{version}.json"));
        let client_jar = folder.join(format!("{version}.jar"));
        for path in [&version_json, &client_jar] {
            if !path.is_file() {
                return Err(SetupError::MissingVersionFile {
                    path: path.clone(),
                    version: version.to_owned(),
                });
            }
        }

        let text = std::fs::read_to_string(&version_json).map_err(|source| SetupError::Io {
            path: version_json.clone(),
            source,
        })?;
        let parsed: VersionJson =
            serde_json::from_str(&text).map_err(|source| SetupError::BadVersionJson {
                path: version_json.clone(),
                source,
            })?;

        Ok(Self {
            dir: dir.to_path_buf(),
            version_json,
            client_jar,
            client_sha1: parsed.downloads.client.and_then(|a| a.sha1),
            server_sha1: parsed.downloads.server.and_then(|a| a.sha1),
            java_major: parsed.java_version.as_ref().and_then(|j| j.major_version),
            java_component: parsed.java_version.and_then(|j| j.component),
        })
    }

    /// Runtimes the official launcher downloaded for this version, if any.
    pub fn launcher_runtimes(&self) -> Vec<PathBuf> {
        let Some(component) = &self.java_component else {
            return Vec::new();
        };
        let base = self.dir.join("runtime").join(component);
        let Ok(platforms) = std::fs::read_dir(&base) else {
            return Vec::new();
        };
        let mut homes: Vec<PathBuf> = platforms
            .flatten()
            .map(|platform| platform.path().join(component))
            .filter(|home| home.is_dir())
            .collect();
        homes.sort();
        homes
    }
}

pub fn sha1_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn fake_install(json: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("versions/26.3");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("26.3.json"), json).unwrap();
        fs::write(folder.join("26.3.jar"), b"jar").unwrap();
        dir
    }

    #[test]
    fn reads_hashes_and_java_requirements() {
        let dir = fake_install(
            r#"{
                "downloads": {
                    "client": {"sha1": "aaa", "size": 1},
                    "server": {"sha1": "bbb"}
                },
                "javaVersion": {"component": "java-runtime-epsilon", "majorVersion": 25}
            }"#,
        );
        let install = Install::inspect(dir.path()).unwrap();
        assert_eq!(install.client_sha1.as_deref(), Some("aaa"));
        assert_eq!(install.server_sha1.as_deref(), Some("bbb"));
        assert_eq!(install.java_major, Some(25));
        assert_eq!(
            install.java_component.as_deref(),
            Some("java-runtime-epsilon")
        );
    }

    #[test]
    fn tolerates_missing_optional_sections() {
        let dir = fake_install("{}");
        let install = Install::inspect(dir.path()).unwrap();
        assert!(install.server_sha1.is_none() && install.java_major.is_none());
    }

    #[test]
    fn missing_version_is_reported_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let err = Install::inspect(dir.path()).unwrap_err();
        assert!(matches!(err, SetupError::MissingVersionFile { .. }));
        assert!(err.to_string().contains("26.3"));
    }

    #[test]
    fn invalid_json_is_reported() {
        let dir = fake_install("{ not json");
        assert!(matches!(
            Install::inspect(dir.path()),
            Err(SetupError::BadVersionJson { .. })
        ));
    }

    #[test]
    fn finds_launcher_runtimes() {
        let dir = fake_install(
            r#"{"javaVersion":{"component":"java-runtime-epsilon","majorVersion":25}}"#,
        );
        let home = dir
            .path()
            .join("runtime/java-runtime-epsilon/windows-x64/java-runtime-epsilon");
        fs::create_dir_all(&home).unwrap();
        let install = Install::inspect(dir.path()).unwrap();
        assert_eq!(install.launcher_runtimes(), vec![home]);
    }

    #[test]
    fn sha1_matches_known_vector() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("abc");
        fs::write(&file, b"abc").unwrap();
        assert_eq!(
            sha1_file(&file).unwrap(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }
}
