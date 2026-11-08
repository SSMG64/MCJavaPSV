use std::io::{Read, Seek, SeekFrom};

use crate::error::PackError;
use crate::{Codec, HEADER_LEN, MAGIC, MAX_ENTRY_SIZE, MAX_PATH_LEN, VERSION};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub path: String,
    pub codec: Codec,
    pub raw_size: u32,
    pub stored_size: u32,
    pub offset: u64,
    pub crc: u32,
}

/// Opens a pack by reading only its header and index. Entries are read on demand.
#[derive(Debug)]
pub struct PackReader<R: Read + Seek> {
    source: R,
    entries: Vec<IndexEntry>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], PackError> {
        let end = self
            .at
            .checked_add(count)
            .filter(|&end| end <= self.bytes.len());
        let end = end.ok_or(PackError::Corrupt("the index is truncated"))?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn u16(&mut self) -> Result<u16, PackError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, PackError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, PackError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
}

impl<R: Read + Seek> PackReader<R> {
    pub fn open(mut source: R) -> Result<Self, PackError> {
        let file_len = source.seek(SeekFrom::End(0))?;
        if file_len < HEADER_LEN as u64 {
            return Err(PackError::Corrupt("the file is shorter than a header"));
        }
        source.seek(SeekFrom::Start(0))?;
        let mut header = [0_u8; HEADER_LEN];
        source.read_exact(&mut header)?;

        if header[0..4] != MAGIC {
            return Err(PackError::Corrupt("bad magic"));
        }
        let mut head = Cursor {
            bytes: &header,
            at: 4,
        };
        let version = head.u16()?;
        let _flags = head.u16()?;
        let count = head.u32()?;
        let index_offset = head.u64()?;
        let index_size = head.u32()?;
        let index_crc = head.u32()?;
        let header_crc = head.u32()?;
        if crc32fast::hash(&header[..HEADER_LEN - 4]) != header_crc {
            return Err(PackError::Corrupt("the header checksum does not match"));
        }
        if version > VERSION {
            return Err(PackError::UnsupportedVersion(version));
        }

        let index_end = index_offset
            .checked_add(u64::from(index_size))
            .filter(|&end| end <= file_len && index_offset >= HEADER_LEN as u64)
            .ok_or(PackError::Corrupt("the index lies outside the file"))?;
        let _ = index_end;

        source.seek(SeekFrom::Start(index_offset))?;
        let mut index = vec![0_u8; index_size as usize];
        source.read_exact(&mut index)?;
        if crc32fast::hash(&index) != index_crc {
            return Err(PackError::Corrupt("the index checksum does not match"));
        }

        let mut cursor = Cursor {
            bytes: &index,
            at: 0,
        };
        let mut entries: Vec<IndexEntry> = Vec::with_capacity((count as usize).min(1 << 20));
        for _ in 0..count {
            let path_len = usize::from(cursor.u16()?);
            if path_len == 0 || path_len > MAX_PATH_LEN {
                return Err(PackError::Corrupt("an entry path has a bad length"));
            }
            let path = std::str::from_utf8(cursor.take(path_len)?)
                .map_err(|_| PackError::Corrupt("an entry path is not UTF-8"))?
                .to_owned();
            let codec = Codec::from_byte(cursor.take(1)?[0])
                .ok_or(PackError::Corrupt("an entry uses an unknown codec"))?;
            let raw_size = cursor.u32()?;
            let stored_size = cursor.u32()?;
            let offset = cursor.u64()?;
            let crc = cursor.u32()?;

            let data_end = offset
                .checked_add(u64::from(stored_size))
                .ok_or(PackError::Corrupt("an entry overflows"))?;
            if offset < HEADER_LEN as u64 || data_end > index_offset {
                return Err(PackError::Corrupt("an entry lies outside the data region"));
            }
            if raw_size > MAX_ENTRY_SIZE || (codec == Codec::Store && raw_size != stored_size) {
                return Err(PackError::Corrupt("an entry has an impossible size"));
            }
            if entries.last().is_some_and(|previous| previous.path >= path) {
                return Err(PackError::Corrupt("the index is not strictly sorted"));
            }
            entries.push(IndexEntry {
                path,
                codec,
                raw_size,
                stored_size,
                offset,
                crc,
            });
        }
        if cursor.at != index.len() {
            return Err(PackError::Corrupt("the index has trailing bytes"));
        }
        Ok(Self { source, entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    pub fn entry(&self, path: &str) -> Option<&IndexEntry> {
        self.entries
            .binary_search_by(|entry| entry.path.as_str().cmp(path))
            .ok()
            .map(|at| &self.entries[at])
    }

    pub fn contains(&self, path: &str) -> bool {
        self.entry(path).is_some()
    }

    /// Reads an entry, decompresses it and checks its CRC.
    pub fn read(&mut self, path: &str) -> Result<Vec<u8>, PackError> {
        let entry = self
            .entry(path)
            .cloned()
            .ok_or_else(|| PackError::NotFound(path.to_owned()))?;
        self.read_entry(&entry)
    }

    pub fn read_entry(&mut self, entry: &IndexEntry) -> Result<Vec<u8>, PackError> {
        self.source.seek(SeekFrom::Start(entry.offset))?;
        let mut stored = vec![0_u8; entry.stored_size as usize];
        self.source.read_exact(&mut stored)?;

        let raw = match entry.codec {
            Codec::Store => stored,
            Codec::Lz4 => lz4_flex::block::decompress(&stored, entry.raw_size as usize)
                .map_err(|_| PackError::Decompress(entry.path.clone()))?,
        };
        if raw.len() != entry.raw_size as usize {
            return Err(PackError::Decompress(entry.path.clone()));
        }
        if crc32fast::hash(&raw) != entry.crc {
            return Err(PackError::Checksum(entry.path.clone()));
        }
        Ok(raw)
    }

    pub fn into_inner(self) -> R {
        self.source
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor as IoCursor;

    use super::*;
    use crate::writer::{CodecChoice, PackWriter};

    fn sample(entries: &[(&str, Vec<u8>, CodecChoice)]) -> Vec<u8> {
        let mut writer = PackWriter::new(IoCursor::new(Vec::new())).unwrap();
        for (path, data, choice) in entries {
            writer.add(path, data, *choice).unwrap();
        }
        writer.finish().unwrap().0.into_inner()
    }

    fn pseudo_random(len: usize) -> Vec<u8> {
        let mut state = 0x1234_5678_u32;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state >> 8) as u8
            })
            .collect()
    }

    #[test]
    fn round_trips_stored_and_compressed_entries() {
        let text = b"the quick brown fox ".repeat(100);
        let noise = pseudo_random(500);
        let bytes = sample(&[
            ("a/text.txt", text.clone(), CodecChoice::Auto),
            ("a/noise.bin", noise.clone(), CodecChoice::Auto),
            ("b/forced.ogg", text.clone(), CodecChoice::Store),
            ("tiny", b"x".to_vec(), CodecChoice::Auto),
        ]);
        let mut pack = PackReader::open(IoCursor::new(bytes)).unwrap();
        assert_eq!(pack.len(), 4);
        assert_eq!(pack.read("a/text.txt").unwrap(), text);
        assert_eq!(pack.read("a/noise.bin").unwrap(), noise);
        assert_eq!(pack.read("b/forced.ogg").unwrap(), text);
        assert_eq!(pack.read("tiny").unwrap(), b"x");

        assert_eq!(pack.entry("a/text.txt").unwrap().codec, Codec::Lz4);
        assert_eq!(pack.entry("a/noise.bin").unwrap().codec, Codec::Store);
        assert_eq!(pack.entry("b/forced.ogg").unwrap().codec, Codec::Store);
        assert_eq!(pack.entry("tiny").unwrap().codec, Codec::Store);
    }

    #[test]
    fn index_is_sorted_and_lookup_finds_every_entry() {
        let names: Vec<String> = (0..500).rev().map(|i| format!("dir/{i:04}.bin")).collect();
        let entries: Vec<_> = names
            .iter()
            .map(|n| (n.as_str(), n.as_bytes().to_vec(), CodecChoice::Auto))
            .collect();
        let bytes = sample(&entries);
        let mut pack = PackReader::open(IoCursor::new(bytes)).unwrap();
        let listed: Vec<_> = pack.entries().iter().map(|e| e.path.clone()).collect();
        let mut sorted = listed.clone();
        sorted.sort();
        assert_eq!(listed, sorted);
        for name in &names {
            assert_eq!(pack.read(name).unwrap(), name.as_bytes());
        }
        assert!(!pack.contains("dir/9999.bin"));
        assert!(matches!(pack.read("nope"), Err(PackError::NotFound(_))));
    }

    #[test]
    fn empty_pack_and_empty_entries_work() {
        let bytes = sample(&[]);
        assert!(PackReader::open(IoCursor::new(bytes)).unwrap().is_empty());

        let bytes = sample(&[("empty", Vec::new(), CodecChoice::Auto)]);
        let mut pack = PackReader::open(IoCursor::new(bytes)).unwrap();
        assert_eq!(pack.read("empty").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn output_is_deterministic() {
        let make = || {
            sample(&[
                ("z", pseudo_random(300), CodecChoice::Auto),
                ("a", b"abc".repeat(200), CodecChoice::Auto),
            ])
        };
        assert_eq!(make(), make());
    }

    #[test]
    fn writer_rejects_bad_input() {
        let mut writer = PackWriter::new(IoCursor::new(Vec::new())).unwrap();
        writer.add("ok", b"1", CodecChoice::Store).unwrap();
        assert!(matches!(
            writer.add("ok", b"2", CodecChoice::Store),
            Err(PackError::Duplicate(_))
        ));
        for path in ["", "/abs", "dir/", "a/../b", "a//b", "a\\b", "c:x", "bad?"] {
            assert!(
                matches!(
                    writer.add(path, b"1", CodecChoice::Store),
                    Err(PackError::InvalidPath { .. })
                ),
                "{path:?}"
            );
        }
    }

    #[test]
    fn corrupted_data_is_caught_by_the_checksum() {
        let mut bytes = sample(&[(
            "a",
            b"hello world, hello world".to_vec(),
            CodecChoice::Store,
        )]);
        bytes[HEADER_LEN] ^= 0xFF;
        let mut pack = PackReader::open(IoCursor::new(bytes)).unwrap();
        assert!(matches!(pack.read("a"), Err(PackError::Checksum(_))));
    }

    #[test]
    fn corrupted_index_header_and_truncation_are_rejected() {
        let good = sample(&[("a", b"data".repeat(50), CodecChoice::Auto)]);

        let mut bad_header = good.clone();
        bad_header[10] ^= 1;
        assert!(matches!(
            PackReader::open(IoCursor::new(bad_header)),
            Err(PackError::Corrupt(_))
        ));

        let mut bad_index = good.clone();
        let last = bad_index.len() - 1;
        bad_index[last] ^= 1;
        assert!(matches!(
            PackReader::open(IoCursor::new(bad_index)),
            Err(PackError::Corrupt(_))
        ));

        for cut in [0, 10, HEADER_LEN, good.len() - 1] {
            assert!(
                PackReader::open(IoCursor::new(good[..cut].to_vec())).is_err(),
                "cut at {cut}"
            );
        }
        assert!(matches!(
            PackReader::open(IoCursor::new(b"NOPE".repeat(20))),
            Err(PackError::Corrupt(_))
        ));
    }

    #[test]
    fn newer_versions_are_refused() {
        let mut bytes = sample(&[]);
        bytes[4..6].copy_from_slice(&(VERSION + 1).to_le_bytes());
        let crc = crc32fast::hash(&bytes[..HEADER_LEN - 4]);
        bytes[HEADER_LEN - 4..HEADER_LEN].copy_from_slice(&crc.to_le_bytes());
        assert!(matches!(
            PackReader::open(IoCursor::new(bytes)),
            Err(PackError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn stats_describe_the_pack() {
        let mut writer = PackWriter::new(IoCursor::new(Vec::new())).unwrap();
        writer
            .add("a", &b"abcd".repeat(100), CodecChoice::Auto)
            .unwrap();
        writer.add("b", b"xyz", CodecChoice::Store).unwrap();
        let (out, stats) = writer.finish().unwrap();
        assert_eq!(stats.entries, 2);
        assert_eq!(stats.lz4_entries, 1);
        assert_eq!(stats.raw_bytes, 403);
        assert!(stats.stored_bytes < stats.raw_bytes);
        assert_eq!(stats.file_bytes, out.into_inner().len() as u64);
    }

    #[test]
    fn works_on_a_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.pak");
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = PackWriter::new(file).unwrap();
        writer
            .add("hello.txt", &b"hello ".repeat(50), CodecChoice::Auto)
            .unwrap();
        writer.finish().unwrap();
        let mut pack = PackReader::open(std::fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(pack.read("hello.txt").unwrap(), b"hello ".repeat(50));
    }
}
