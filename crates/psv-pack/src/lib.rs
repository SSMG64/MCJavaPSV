//! The data format shared by the Windows baker and the Vita runtime.
//!
//! A *pack* is one file holding many named entries, each stored or LZ4 compressed and
//! protected by a CRC. Its index sits at the end, so a writer can stream entries without
//! knowing their sizes, and a reader loads only the index into memory. A *texture* is the
//! lossless replacement for a PNG that the Vita can unpack without an inflate step.

pub mod error;
pub mod reader;
pub mod texture;
pub mod writer;

pub use error::PackError;
pub use reader::{IndexEntry, PackReader};
pub use texture::{Texture, TextureError};
pub use writer::{CodecChoice, PackStats, PackWriter};

pub const MAGIC: [u8; 4] = *b"MCPK";
pub const VERSION: u16 = 1;
pub const HEADER_LEN: usize = 32;
pub const MAX_ENTRY_SIZE: u32 = 1 << 30;
pub const MAX_PATH_LEN: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Store = 0,
    Lz4 = 1,
}

impl Codec {
    pub(crate) fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Store),
            1 => Some(Self::Lz4),
            _ => None,
        }
    }
}

/// Entry paths are relative, use `/`, and avoid anything FAT or Windows cannot store.
pub fn validate_path(path: &str) -> Result<(), PackError> {
    let invalid = |reason| PackError::InvalidPath {
        path: path.to_owned(),
        reason,
    };
    if path.is_empty() {
        return Err(invalid("empty"));
    }
    if path.len() > MAX_PATH_LEN {
        return Err(invalid("too long"));
    }
    if path.starts_with('/') || path.ends_with('/') {
        return Err(invalid("must be relative and name a file"));
    }
    if path.contains(['\\', ':', '*', '?', '"', '<', '>', '|'])
        || path.chars().any(char::is_control)
    {
        return Err(invalid("contains a character FAT and Windows cannot store"));
    }
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(invalid("contains an empty or dot component"));
        }
    }
    Ok(())
}
