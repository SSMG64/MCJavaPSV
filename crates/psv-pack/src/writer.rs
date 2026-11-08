use std::collections::HashSet;
use std::io::{Seek, SeekFrom, Write};

use crate::error::PackError;
use crate::{Codec, HEADER_LEN, MAGIC, MAX_ENTRY_SIZE, VERSION, validate_path};

const MIN_COMPRESSIBLE: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecChoice {
    /// Keep the bytes as they are. For data that is already compressed.
    Store,
    /// Use LZ4 when it saves at least a sixteenth of the size.
    Auto,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackStats {
    pub entries: u32,
    pub lz4_entries: u32,
    pub raw_bytes: u64,
    pub stored_bytes: u64,
    pub file_bytes: u64,
}

#[derive(Debug)]
struct Pending {
    path: String,
    codec: Codec,
    raw_size: u32,
    stored_size: u32,
    offset: u64,
    crc: u32,
}

/// Streams entries into a pack and writes the index when finished.
#[derive(Debug)]
pub struct PackWriter<W: Write + Seek> {
    out: W,
    position: u64,
    entries: Vec<Pending>,
    seen: HashSet<String>,
}

impl<W: Write + Seek> PackWriter<W> {
    pub fn new(mut out: W) -> Result<Self, PackError> {
        out.write_all(&[0; HEADER_LEN])?;
        Ok(Self {
            out,
            position: HEADER_LEN as u64,
            entries: Vec::new(),
            seen: HashSet::new(),
        })
    }

    pub fn add(
        &mut self,
        path: &str,
        data: &[u8],
        choice: CodecChoice,
    ) -> Result<Codec, PackError> {
        validate_path(path)?;
        if self.seen.contains(path) {
            return Err(PackError::Duplicate(path.to_owned()));
        }
        if data.len() as u64 > u64::from(MAX_ENTRY_SIZE) {
            return Err(PackError::TooLarge {
                path: path.to_owned(),
                size: data.len() as u64,
                max: u64::from(MAX_ENTRY_SIZE),
            });
        }

        let compressed = (choice == CodecChoice::Auto && data.len() >= MIN_COMPRESSIBLE)
            .then(|| lz4_flex::block::compress(data))
            .filter(|packed| packed.len() + data.len() / 16 < data.len());
        let (codec, stored) = match &compressed {
            Some(packed) => (Codec::Lz4, packed.as_slice()),
            None => (Codec::Store, data),
        };

        self.out.write_all(stored)?;
        self.entries.push(Pending {
            path: path.to_owned(),
            codec,
            raw_size: data.len() as u32,
            stored_size: stored.len() as u32,
            offset: self.position,
            crc: crc32fast::hash(data),
        });
        self.seen.insert(path.to_owned());
        self.position += stored.len() as u64;
        Ok(codec)
    }

    /// Writes the index and the header, and returns the sink with what was written.
    pub fn finish(mut self) -> Result<(W, PackStats), PackError> {
        let count = u32::try_from(self.entries.len()).map_err(|_| PackError::TooManyEntries)?;
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));

        let mut index = Vec::new();
        let mut stats = PackStats {
            entries: count,
            ..PackStats::default()
        };
        for entry in &self.entries {
            index.extend_from_slice(&(entry.path.len() as u16).to_le_bytes());
            index.extend_from_slice(entry.path.as_bytes());
            index.push(entry.codec as u8);
            index.extend_from_slice(&entry.raw_size.to_le_bytes());
            index.extend_from_slice(&entry.stored_size.to_le_bytes());
            index.extend_from_slice(&entry.offset.to_le_bytes());
            index.extend_from_slice(&entry.crc.to_le_bytes());
            stats.raw_bytes += u64::from(entry.raw_size);
            stats.stored_bytes += u64::from(entry.stored_size);
            stats.lz4_entries += u32::from(entry.codec == Codec::Lz4);
        }

        let index_offset = self.position;
        self.out.write_all(&index)?;
        stats.file_bytes = index_offset + index.len() as u64;

        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend_from_slice(&MAGIC);
        header.extend_from_slice(&VERSION.to_le_bytes());
        header.extend_from_slice(&0_u16.to_le_bytes());
        header.extend_from_slice(&count.to_le_bytes());
        header.extend_from_slice(&index_offset.to_le_bytes());
        header.extend_from_slice(&(index.len() as u32).to_le_bytes());
        header.extend_from_slice(&crc32fast::hash(&index).to_le_bytes());
        header.extend_from_slice(&crc32fast::hash(&header).to_le_bytes());

        self.out.seek(SeekFrom::Start(0))?;
        self.out.write_all(&header)?;
        self.out.seek(SeekFrom::Start(stats.file_bytes))?;
        self.out.flush()?;
        Ok((self.out, stats))
    }
}
