use std::io;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SetupError {
    #[error("no Minecraft folder was given and none was found at the default location")]
    NoMinecraftDir,
    #[error("{} is missing; install version {version} from the official launcher first", path.display())]
    MissingVersionFile { path: PathBuf, version: String },
    #[error("could not read {}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("{} is not a valid version file: {source}", path.display())]
    BadVersionJson {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error(
        "no Java {major} was found; install a Java {major} JDK (for example Eclipse Temurin) or point the app at one"
    )]
    JavaNotFound { major: u32 },
    #[error("could not start {}: {source}", program.display())]
    Spawn { program: PathBuf, source: io::Error },
    #[error("horizon-prepare failed: {0}")]
    PrepareFailed(String),
    #[error("cancelled")]
    Cancelled,
}
