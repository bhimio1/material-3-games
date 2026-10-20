//! Valve's binary KeyValues, as used by `shortcuts.vdf` and the
//! `appcache/stats/*.bin` achievement caches.
//!
//! Each entry is a type byte, a NUL-terminated key, then the value. Maps
//! hold entries until an end byte (0x08). Everything we don't understand is
//! kept as-is, so a read/write round trip is lossless.

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Map(Vec<(String, Value)>),
    /// Raw bytes (usually UTF-8, but Steam doesn't promise it).
    Str(Vec<u8>),
    Int(i32),
    Float(f32),
    Ptr(i32),
    /// UTF-16 string, without the terminator.
    WStr(Vec<u16>),
    Color(i32),
    U64(u64),
    I64(i64),
}

const MAP: u8 = 0x00;
const STR: u8 = 0x01;
const INT: u8 = 0x02;
const FLOAT: u8 = 0x03;
const PTR: u8 = 0x04;
const WSTR: u8 = 0x05;
const COLOR: u8 = 0x06;
const U64: u8 = 0x07;
const END: u8 = 0x08;
// Newer appinfo-style files end maps with 0x0B.
const END2: u8 = 0x0B;
const I64: u8 = 0x0A;

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(s.as_bytes().to_vec())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(e) => e.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self {
            Value::Map(e) => e.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn path(&self, keys: &[&str]) -> Option<&Value> {
        keys.iter().try_fold(self, |v, k| v.get(k))
    }

    pub fn entries(&self) -> &[(String, Value)] {
        match self {
            Value::Map(e) => e,
            _ => &[],
        }
    }

    pub fn as_str(&self) -> Option<String> {
        match self {
            Value::Str(b) => Some(String::from_utf8_lossy(b).into_owned()),
            Value::WStr(w) => Some(String::from_utf16_lossy(w)),
            _ => None,
        }
    }

    /// Any integer type as i64.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) | Value::Ptr(i) | Value::Color(i) => Some(*i as i64),
            Value::U64(u) => Some(*u as i64),
            Value::I64(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            Value::Str(b) => std::str::from_utf8(b).ok()?.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn get_str(&self, key: &str) -> Option<String> {
        self.get(key).and_then(Value::as_str)
    }

    pub fn get_int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(Value::as_int)
    }

    /// Set (or add) `key` in a map.
    pub fn set(&mut self, key: &str, value: Value) {
        if let Value::Map(e) = self {
            if let Some(slot) = e.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
                slot.1 = value;
            } else {
                e.push((key.to_string(), value));
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let end = self.pos.checked_add(n).filter(|e| *e <= self.data.len()).ok_or_else(|| Error("unexpected end of file".into()))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn cstr(&mut self) -> Result<Vec<u8>, Error> {
        let rest = &self.data[self.pos..];
        let nul = rest.iter().position(|b| *b == 0).ok_or_else(|| Error("unterminated string".into()))?;
        let s = rest[..nul].to_vec();
        self.pos += nul + 1;
        Ok(s)
    }

    fn wstr(&mut self) -> Result<Vec<u16>, Error> {
        let mut out = Vec::new();
        loop {
            let b = self.take(2)?;
            let c = u16::from_le_bytes([b[0], b[1]]);
            if c == 0 {
                return Ok(out);
            }
            out.push(c);
        }
    }

    fn map(&mut self, depth: usize) -> Result<Vec<(String, Value)>, Error> {
        if depth > 64 {
            return Err(Error("nested too deeply".into()));
        }
        let mut entries = Vec::new();
        loop {
            let Some(t) = self.byte() else {
                // Top level may simply end.
                return if depth == 0 { Ok(entries) } else { Err(Error("unexpected end of file".into())) };
            };
            if t == END || t == END2 {
                return Ok(entries);
            }
            let key = String::from_utf8_lossy(&self.cstr()?).into_owned();
            let value = match t {
                MAP => Value::Map(self.map(depth + 1)?),
                STR => Value::Str(self.cstr()?),
                INT => Value::Int(i32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes"))),
                FLOAT => Value::Float(f32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes"))),
                PTR => Value::Ptr(i32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes"))),
                WSTR => Value::WStr(self.wstr()?),
                COLOR => Value::Color(i32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes"))),
                U64 => Value::U64(u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes"))),
                I64 => Value::I64(i64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes"))),
                other => return Err(Error(format!("unknown type 0x{other:02x} at byte {}", self.pos))),
            };
            entries.push((key, value));
        }
    }
}

/// Parse a whole file into a map of its top-level entries.
pub fn parse(data: &[u8]) -> Result<Value, Error> {
    let mut r = Reader { data, pos: 0 };
    let entries = r.map(0)?;
    Ok(Value::Map(entries))
}

fn write_entries(out: &mut Vec<u8>, entries: &[(String, Value)]) {
    for (k, v) in entries {
        let t = match v {
            Value::Map(_) => MAP,
            Value::Str(_) => STR,
            Value::Int(_) => INT,
            Value::Float(_) => FLOAT,
            Value::Ptr(_) => PTR,
            Value::WStr(_) => WSTR,
            Value::Color(_) => COLOR,
            Value::U64(_) => U64,
            Value::I64(_) => I64,
        };
        out.push(t);
        out.extend_from_slice(k.as_bytes());
        out.push(0);
        match v {
            Value::Map(inner) => {
                write_entries(out, inner);
                out.push(END);
            }
            Value::Str(b) => {
                out.extend(b.iter().copied().filter(|c| *c != 0));
                out.push(0);
            }
            Value::Int(i) | Value::Ptr(i) | Value::Color(i) => out.extend_from_slice(&i.to_le_bytes()),
            Value::Float(f) => out.extend_from_slice(&f.to_le_bytes()),
            Value::WStr(w) => {
                for c in w {
                    out.extend_from_slice(&c.to_le_bytes());
                }
                out.extend_from_slice(&[0, 0]);
            }
            Value::U64(u) => out.extend_from_slice(&u.to_le_bytes()),
            Value::I64(i) => out.extend_from_slice(&i.to_le_bytes()),
        }
    }
}

/// Serialize a document produced by [`parse`]. The top level ends with an
/// end byte, as Steam writes it.
pub fn write(doc: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write_entries(&mut out, doc.entries());
    out.push(END);
    out
}

/// A shortcuts.vdf with one entry, built byte by byte per the format.
#[cfg(test)]
pub fn tests_fixture() -> Vec<u8> {
    tests::fixture()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn fixture() -> Vec<u8> {
        let mut b = Vec::new();
        b.push(0x00);
        b.extend_from_slice(b"shortcuts\0");
        b.push(0x00);
        b.extend_from_slice(b"0\0");
        b.push(0x02);
        b.extend_from_slice(b"appid\0");
        b.extend_from_slice(&(-1_234_567_i32).to_le_bytes());
        b.push(0x01);
        b.extend_from_slice(b"AppName\0Quake\0");
        b.push(0x01);
        b.extend_from_slice(b"Exe\0\"/usr/bin/quake\"\0");
        b.push(0x02);
        b.extend_from_slice(b"AllowOverlay\0");
        b.extend_from_slice(&1_i32.to_le_bytes());
        b.push(0x00);
        b.extend_from_slice(b"tags\0");
        b.push(0x01);
        b.extend_from_slice(b"0\0favorite\0");
        b.push(0x08); // end tags
        b.push(0x07);
        b.extend_from_slice(b"UnknownFuture\0");
        b.extend_from_slice(&42_u64.to_le_bytes());
        b.push(0x08); // end "0"
        b.push(0x08); // end shortcuts
        b.push(0x08); // end document
        b
    }

    #[test]
    fn parses_and_round_trips() {
        let data = fixture();
        let doc = parse(&data).unwrap();
        let s = doc.path(&["shortcuts", "0"]).unwrap();
        assert_eq!(s.get_int("appid"), Some(-1_234_567));
        assert_eq!(s.get_str("AppName").as_deref(), Some("Quake"));
        assert_eq!(s.get_str("exe").as_deref(), Some("\"/usr/bin/quake\""));
        assert_eq!(s.path(&["tags", "0"]).and_then(Value::as_str).as_deref(), Some("favorite"));
        assert_eq!(s.get_int("UnknownFuture"), Some(42));
        assert_eq!(write(&doc), data);
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        let data = fixture();
        for cut in 1..data.len() - 1 {
            let _ = parse(&data[..cut]);
        }
        assert!(parse(&[0x00, b'a']).is_err());
        assert!(parse(&[0x42, b'a', 0]).is_err());
        assert_eq!(parse(&[]).unwrap(), Value::Map(vec![]));
    }

    #[test]
    fn all_types() {
        let doc = Value::Map(vec![(
            "m".into(),
            Value::Map(vec![
                ("f".into(), Value::Float(1.5)),
                ("w".into(), Value::WStr("hé".encode_utf16().collect())),
                ("c".into(), Value::Color(7)),
                ("p".into(), Value::Ptr(9)),
                ("i".into(), Value::I64(-5)),
            ]),
        )]);
        let back = parse(&write(&doc)).unwrap();
        assert_eq!(back, doc);
        assert_eq!(back.path(&["m", "w"]).and_then(Value::as_str).as_deref(), Some("hé"));
    }
}
