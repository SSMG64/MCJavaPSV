use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub minecraft_dir: Option<PathBuf>,
    pub server_jar: Option<PathBuf>,
    pub java_home: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
}

impl Settings {
    /// Missing or unreadable settings fall back to defaults; they are only conveniences.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
                tracing::warn!(
                    "ignoring unreadable settings file {}: {error}",
                    path.display()
                );
                Self::default()
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                tracing::warn!("could not read settings {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, path)
    }

    pub fn inputs(&self, payload: &crate::payload::Payload) -> psv_setup::report::Inputs {
        psv_setup::report::Inputs {
            minecraft_dir: self.minecraft_dir.clone(),
            server_jar: self.server_jar.clone(),
            java_home: self.java_home.clone(),
            prepare_exe: Some(payload.prepare_exe()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        let settings = Settings {
            server_jar: Some("a/b.jar".into()),
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn missing_and_corrupt_files_give_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Settings::load(&dir.path().join("none.json")),
            Settings::default()
        );
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "{ nope").unwrap();
        assert_eq!(Settings::load(&bad), Settings::default());
    }

    #[test]
    fn unknown_and_missing_fields_are_tolerated() {
        let parsed: Settings = serde_json::from_str(r#"{"server_jar":"x","future":1}"#).unwrap();
        assert_eq!(parsed.server_jar, Some("x".into()));
        assert!(parsed.minecraft_dir.is_none());
    }
}
