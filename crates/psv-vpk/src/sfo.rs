use std::collections::BTreeMap;

use thiserror::Error;

const MAGIC: u32 = 0x4653_5000;
const VERSION: u32 = 0x0000_0101;
const HEADER_LEN: usize = 20;
const ENTRY_LEN: usize = 16;
const ENTRY_ALIGNMENT: u8 = 4;
const TYPE_TEXT: u8 = 2;
const TYPE_INTEGER: u8 = 4;

const TITLE_RESERVED: u32 = 0x80;
const SHORT_TITLE_RESERVED: u32 = 52;
const CONTENT_ID_RESERVED: u32 = 48;
const ATTRIBUTE_NO_COMMUNICATION_ZONE: u32 = 0x8000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SfoError {
    #[error("title id must be exactly 9 uppercase ASCII letters or digits, got {0:?}")]
    InvalidTitleId(String),
    #[error("{field} is {len} bytes but only {max} fit")]
    TooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },
    #[error("the value for {0} contains a NUL byte")]
    EmbeddedNul(String),
    #[error("not a valid PARAM.SFO: {0}")]
    Malformed(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text { text: String, reserved: u32 },
    Integer(u32),
}

/// Memory mode flags for the `ATTRIBUTE2` field, named as `vita-mksfoex` names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MemoryMode {
    Standard = 0,
    Mem29 = 4,
    Mem77 = 8,
    #[default]
    Mem109 = 12,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSfo {
    entries: BTreeMap<String, Value>,
}

impl ParamSfo {
    /// The defaults `vita-mksfoex` applies for a homebrew application.
    pub fn homebrew(title_id: &str, title: &str) -> Result<Self, SfoError> {
        validate_title_id(title_id)?;
        check_text("TITLE", title, TITLE_RESERVED)?;

        let mut sfo = Self {
            entries: BTreeMap::new(),
        };
        sfo.set_text("APP_VER", "01.00", 0)?;
        sfo.set_integer("ATTRIBUTE", ATTRIBUTE_NO_COMMUNICATION_ZONE);
        sfo.set_integer("ATTRIBUTE2", MemoryMode::default() as u32);
        sfo.set_integer("ATTRIBUTE_MINOR", 0x10);
        sfo.set_text("CATEGORY", "gd", 0)?;
        sfo.set_text(
            "CONTENT_ID",
            &format!("HB0001-{title_id}_00-0000000000000000"),
            CONTENT_ID_RESERVED,
        )?;
        sfo.set_integer("GC_RO_SIZE", 0);
        sfo.set_integer("GC_RW_SIZE", 0);
        sfo.set_integer("PARENTAL_LEVEL", 0);
        sfo.set_text("PSP2_DISP_VER", "00.000", 0)?;
        sfo.set_integer("PSP2_SYSTEM_VER", 0);
        sfo.set_integer("REGION_DENY", 0);
        sfo.set_integer("SAVEDATA_MAX_SIZE", 1_048_576);
        sfo.set_text(
            "STITLE",
            truncate_to_boundary(title, SHORT_TITLE_RESERVED as usize - 1),
            SHORT_TITLE_RESERVED,
        )?;
        sfo.set_text("TITLE", title, TITLE_RESERVED)?;
        sfo.set_text("TITLE_ID", title_id, 0)?;
        sfo.set_text("VERSION", "01.00", 0)?;
        Ok(sfo)
    }

    pub fn with_memory_mode(mut self, mode: MemoryMode) -> Self {
        self.set_integer("ATTRIBUTE2", mode as u32);
        self
    }

    pub fn with_version(mut self, version: &str) -> Result<Self, SfoError> {
        self.set_text("APP_VER", version, 0)?;
        self.set_text("VERSION", version, 0)?;
        Ok(self)
    }

    pub fn set_text(&mut self, key: &str, text: &str, reserved: u32) -> Result<(), SfoError> {
        if text.contains('\0') {
            return Err(SfoError::EmbeddedNul(key.to_owned()));
        }
        if reserved != 0 {
            check_text(static_name(key), text, reserved)?;
        }
        self.entries.insert(
            key.to_owned(),
            Value::Text {
                text: text.to_owned(),
                reserved,
            },
        );
        Ok(())
    }

    pub fn set_integer(&mut self, key: &str, value: u32) {
        self.entries.insert(key.to_owned(), Value::Integer(value));
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.get(key)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut index = Vec::with_capacity(self.entries.len() * ENTRY_LEN);
        let mut keys: Vec<u8> = Vec::new();
        let mut data: Vec<u8> = Vec::new();

        for (key, value) in &self.entries {
            let name_offset = keys.len() as u16;
            let data_offset = data.len() as u32;
            keys.extend_from_slice(key.as_bytes());
            keys.push(0);

            let (kind, used, total) = match value {
                Value::Integer(number) => {
                    data.extend_from_slice(&number.to_le_bytes());
                    (TYPE_INTEGER, 4, 4)
                }
                Value::Text { text, reserved } => {
                    let used = text.len() as u32 + 1;
                    let total = if *reserved != 0 {
                        *reserved
                    } else {
                        used.next_multiple_of(4)
                    };
                    data.extend_from_slice(text.as_bytes());
                    data.resize(data_offset as usize + total as usize, 0);
                    (TYPE_TEXT, used, total)
                }
            };

            index.extend_from_slice(&name_offset.to_le_bytes());
            index.push(ENTRY_ALIGNMENT);
            index.push(kind);
            index.extend_from_slice(&used.to_le_bytes());
            index.extend_from_slice(&total.to_le_bytes());
            index.extend_from_slice(&data_offset.to_le_bytes());
        }

        keys.resize(keys.len().next_multiple_of(4), 0);

        let key_offset = (HEADER_LEN + index.len()) as u32;
        let data_table_offset = key_offset + keys.len() as u32;

        let mut out = Vec::with_capacity(data_table_offset as usize + data.len());
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&key_offset.to_le_bytes());
        out.extend_from_slice(&data_table_offset.to_le_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&index);
        out.extend_from_slice(&keys);
        out.extend_from_slice(&data);
        out
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, SfoError> {
        let word = |at: usize| -> Result<u32, SfoError> {
            bytes
                .get(at..at + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .ok_or(SfoError::Malformed("truncated"))
        };
        if word(0)? != MAGIC {
            return Err(SfoError::Malformed("bad magic"));
        }
        let key_table = word(8)? as usize;
        let data_table = word(12)? as usize;
        let count = word(16)? as usize;

        let mut entries = BTreeMap::new();
        for i in 0..count {
            let at = HEADER_LEN + i * ENTRY_LEN;
            let record = bytes
                .get(at..at + ENTRY_LEN)
                .ok_or(SfoError::Malformed("truncated index"))?;
            let name_offset = u16::from_le_bytes([record[0], record[1]]) as usize;
            let kind = record[3];
            let used = word(at + 4)? as usize;
            let total = word(at + 8)?;
            let data_offset = word(at + 12)? as usize;

            let name_start = key_table + name_offset;
            let name_tail = bytes
                .get(name_start..)
                .ok_or(SfoError::Malformed("key offset out of range"))?;
            let name_len = name_tail
                .iter()
                .position(|&b| b == 0)
                .ok_or(SfoError::Malformed("unterminated key"))?;
            let key = std::str::from_utf8(&name_tail[..name_len])
                .map_err(|_| SfoError::Malformed("key is not UTF-8"))?
                .to_owned();

            let value_start = data_table + data_offset;
            let value = match kind {
                TYPE_INTEGER => Value::Integer(word(value_start)?),
                TYPE_TEXT => {
                    let raw = bytes
                        .get(value_start..value_start + used)
                        .ok_or(SfoError::Malformed("value out of range"))?;
                    let text = std::str::from_utf8(raw.strip_suffix(&[0]).unwrap_or(raw))
                        .map_err(|_| SfoError::Malformed("value is not UTF-8"))?
                        .to_owned();
                    Value::Text {
                        text,
                        reserved: total,
                    }
                }
                _ => return Err(SfoError::Malformed("unsupported value type")),
            };
            entries.insert(key, value);
        }
        Ok(Self { entries })
    }
}

fn validate_title_id(id: &str) -> Result<(), SfoError> {
    let valid = id.len() == 9
        && id
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(SfoError::InvalidTitleId(id.to_owned()))
    }
}

fn check_text(field: &'static str, text: &str, reserved: u32) -> Result<(), SfoError> {
    let max = reserved as usize - 1;
    if text.len() > max {
        return Err(SfoError::TooLong {
            field,
            len: text.len(),
            max,
        });
    }
    Ok(())
}

fn static_name(key: &str) -> &'static str {
    match key {
        "TITLE" => "TITLE",
        "STITLE" => "STITLE",
        "CONTENT_ID" => "CONTENT_ID",
        _ => "value",
    }
}

fn truncate_to_boundary(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN_DEFAULT: &[u8] = include_bytes!("../tests/data/golden-default.sfo");
    const GOLDEN_NO_EXTRA_MEMORY: &[u8] =
        include_bytes!("../tests/data/golden-no-extra-memory.sfo");

    #[test]
    fn matches_vita_mksfoex_defaults() {
        let sfo = ParamSfo::homebrew("MCJVPSV01", "MCJavaPSV").unwrap();
        assert_eq!(sfo.to_bytes(), GOLDEN_DEFAULT);
    }

    #[test]
    fn matches_vita_mksfoex_with_overrides() {
        let sfo = ParamSfo::homebrew("MCJVPSV01", "MCJavaPSV")
            .unwrap()
            .with_memory_mode(MemoryMode::Standard)
            .with_version("00.10")
            .unwrap();
        assert_eq!(sfo.to_bytes(), GOLDEN_NO_EXTRA_MEMORY);
    }

    #[test]
    fn parse_round_trips() {
        let parsed = ParamSfo::parse(GOLDEN_DEFAULT).unwrap();
        assert_eq!(parsed.to_bytes(), GOLDEN_DEFAULT);
        assert_eq!(
            parsed.get("TITLE_ID"),
            Some(&Value::Text {
                text: "MCJVPSV01".into(),
                reserved: 12
            })
        );
        assert_eq!(parsed.get("ATTRIBUTE2"), Some(&Value::Integer(12)));
    }

    #[test]
    fn rejects_bad_title_ids() {
        for id in ["", "short", "lowercase1", "TOOLONGID01", "BAD-ID-01"] {
            assert!(matches!(
                ParamSfo::homebrew(id, "x"),
                Err(SfoError::InvalidTitleId(_))
            ));
        }
    }

    #[test]
    fn rejects_overlong_title() {
        let title = "x".repeat(128);
        assert!(matches!(
            ParamSfo::homebrew("MCJVPSV01", &title),
            Err(SfoError::TooLong { field: "TITLE", .. })
        ));
    }

    #[test]
    fn short_title_truncates_on_a_character_boundary() {
        let title = "é".repeat(40);
        let sfo = ParamSfo::homebrew("MCJVPSV01", &title).unwrap();
        let Some(Value::Text { text, reserved }) = sfo.get("STITLE") else {
            panic!("STITLE missing");
        };
        assert_eq!(*reserved, 52);
        assert!(text.len() < 52 && title.starts_with(text.as_str()));
    }

    #[test]
    fn rejects_nul_and_garbage() {
        let mut sfo = ParamSfo::homebrew("MCJVPSV01", "x").unwrap();
        assert!(matches!(
            sfo.set_text("APP_VER", "a\0b", 0),
            Err(SfoError::EmbeddedNul(_))
        ));
        assert!(ParamSfo::parse(b"not an sfo").is_err());
        assert!(ParamSfo::parse(&GOLDEN_DEFAULT[..30]).is_err());
    }
}
