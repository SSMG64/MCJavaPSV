use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PackError {
    #[error("invalid entry path {path:?}: {reason}")]
    InvalidPath { path: String, reason: &'static str },
    #[error("{0:?} is already in the pack")]
    Duplicate(String),
    #[error("{path:?} is {size} bytes, over the {max} byte limit")]
    TooLarge { path: String, size: u64, max: u64 },
    #[error("the pack holds too many entries")]
    TooManyEntries,
    #[error("not a pack file: {0}")]
    Corrupt(&'static str),
    #[error("the pack is version {0}, newer than this reader understands")]
    UnsupportedVersion(u16),
    #[error("{0:?} is not in the pack")]
    NotFound(String),
    #[error("{0:?} failed its checksum")]
    Checksum(String),
    #[error("{0:?} could not be decompressed")]
    Decompress(String),
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
}
