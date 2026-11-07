//! Reader/writer for Underkeep's "DfT" binary container.
//!
//! Layout (all integers little-endian):
//!
//! ```text
//! magic   : FF 44 66 54            ("\xFFDfT")
//! version : u32                    (1)
//! nstr    : u32                    number of strings in the table
//! strings : nstr x (varint len, bytes)
//! root    : table
//!
//! table   : u32 count, then count x entry
//! entry   : u8 key_kind            0 = named key (string index), 1 = array index
//!           varint key
//!           u8 type                1 = i32, 2 = f32, 3 = string (varint index), 4 = table
//!           payload
//! ```
//!
//! Everything is kept in a form that re-serializes byte-for-byte, so an
//! unmodified document writes back identical to what was read.

use std::fmt;

pub const MAGIC: [u8; 4] = [0xFF, b'D', b'f', b'T'];

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error(msg.into()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Index into the document's string table.
    Name(u32),
    /// Position in an array-like table.
    Index(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i32),
    /// Raw f32 bits, kept as bits so round-trips are exact.
    Float(u32),
    /// Index into the document's string table.
    Str(u32),
    Table(Vec<Entry>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub key: Key,
    pub value: Value,
    /// Encoded byte widths of the key varint and (for strings) the value
    /// varint. The game's encoder sometimes pads varints, so we keep them.
    pub key_w: u8,
    pub val_w: u8,
}

impl Entry {
    pub fn new(key: Key, value: Value) -> Entry {
        Entry {
            key,
            value,
            key_w: 1,
            val_w: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub version: u32,
    pub strings: Vec<Vec<u8>>,
    /// Encoded byte width of each string's length prefix. The game sometimes
    /// pads these (e.g. `FF 8D 00`), so we keep the original width.
    pub len_widths: Vec<u8>,
    pub root: Vec<Entry>,
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.p + n > self.d.len() {
            return err(format!("unexpected end of file at offset {}", self.p));
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn varint(&mut self) -> Result<u32> {
        let mut r: u64 = 0;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            r |= ((b & 0x7F) as u64) << shift;
            if b < 0x80 {
                break;
            }
            shift += 7;
            if shift > 35 {
                return err(format!("bad varint at offset {}", self.p));
            }
        }
        u32::try_from(r).or_else(|_| err("varint overflow"))
    }
    fn table(&mut self, nstr: u32, depth: u32) -> Result<Vec<Entry>> {
        if depth > 256 {
            return err("nesting too deep");
        }
        let count = self.u32()?;
        // Each entry takes at least 4 bytes; reject absurd counts early.
        if count as usize > (self.d.len() - self.p) / 3 + 1 {
            return err(format!("bad table size {count} at offset {}", self.p - 4));
        }
        let mut out = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let kind = self.u8()?;
            let ks = self.p;
            let k = self.varint()?;
            let key_w = (self.p - ks) as u8;
            let mut val_w = 1u8;
            let key = match kind {
                0 => {
                    if k >= nstr {
                        return err(format!("key string {k} out of range at offset {}", self.p));
                    }
                    Key::Name(k)
                }
                1 => Key::Index(k),
                _ => return err(format!("unknown key kind {kind} at offset {}", self.p)),
            };
            let ty = self.u8()?;
            let value = match ty {
                1 => Value::Int(self.u32()? as i32),
                2 => Value::Float(self.u32()?),
                3 => {
                    let vs = self.p;
                    let s = self.varint()?;
                    val_w = (self.p - vs) as u8;
                    if s >= nstr {
                        return err(format!("string {s} out of range at offset {}", self.p));
                    }
                    Value::Str(s)
                }
                4 => Value::Table(self.table(nstr, depth + 1)?),
                _ => return err(format!("unknown value type {ty} at offset {}", self.p - 1)),
            };
            out.push(Entry {
                key,
                value,
                key_w,
                val_w,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
fn put_varint(out: &mut Vec<u8>, v: u32) {
    put_varint_w(out, v, 1);
}

/// Write a varint using at least `width` bytes (padding with continuation bytes).
fn put_varint_w(out: &mut Vec<u8>, mut v: u32, width: u8) {
    let mut written = 0u8;
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        written += 1;
        if v == 0 && written >= width {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn put_table(out: &mut Vec<u8>, t: &[Entry]) {
    out.extend_from_slice(&(t.len() as u32).to_le_bytes());
    for e in t {
        match e.key {
            Key::Name(k) => {
                out.push(0);
                put_varint_w(out, k, e.key_w);
            }
            Key::Index(k) => {
                out.push(1);
                put_varint_w(out, k, e.key_w);
            }
        }
        match &e.value {
            Value::Int(v) => {
                out.push(1);
                out.extend_from_slice(&v.to_le_bytes());
            }
            Value::Float(bits) => {
                out.push(2);
                out.extend_from_slice(&bits.to_le_bytes());
            }
            Value::Str(s) => {
                out.push(3);
                put_varint_w(out, *s, e.val_w);
            }
            Value::Table(t) => {
                out.push(4);
                put_table(out, t);
            }
        }
    }
}

impl Doc {
    pub fn parse(d: &[u8]) -> Result<Doc> {
        if d.len() < 12 || d[..4] != MAGIC {
            return err("not an Underkeep data file (bad magic)");
        }
        let mut r = Reader { d, p: 4 };
        let version = r.u32()?;
        let nstr = r.u32()?;
        if nstr as usize > d.len() {
            return err("bad string table size");
        }
        let mut strings = Vec::with_capacity(nstr as usize);
        let mut len_widths = Vec::with_capacity(nstr as usize);
        for _ in 0..nstr {
            let start = r.p;
            let len = r.varint()? as usize;
            len_widths.push((r.p - start) as u8);
            strings.push(r.bytes(len)?.to_vec());
        }
        let root = r.table(nstr, 0)?;
        if r.p != d.len() {
            return err(format!("{} trailing bytes after data", d.len() - r.p));
        }
        Ok(Doc {
            version,
            strings,
            len_widths,
            root,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        for (i, s) in self.strings.iter().enumerate() {
            let w = self.len_widths.get(i).copied().unwrap_or(1);
            put_varint_w(&mut out, s.len() as u32, w);
            out.extend_from_slice(s);
        }
        put_table(&mut out, &self.root);
        out
    }

    pub fn str(&self, idx: u32) -> String {
        self.strings
            .get(idx as usize)
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default()
    }

    /// Index of `s` in the string table, appending it if missing.
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(i) = self.find_str(s) {
            return i;
        }
        self.strings.push(s.as_bytes().to_vec());
        self.len_widths.push(1);
        (self.strings.len() - 1) as u32
    }

    pub fn find_str(&self, s: &str) -> Option<u32> {
        self.strings
            .iter()
            .position(|b| b == s.as_bytes())
            .map(|i| i as u32)
    }

    pub fn key_name(&self, k: Key) -> String {
        match k {
            Key::Name(i) => self.str(i),
            Key::Index(i) => format!("[{i}]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn varint_roundtrip() {
        for v in [0u32, 1, 127, 128, 300, 16383, 16384, u32::MAX] {
            let mut b = Vec::new();
            put_varint(&mut b, v);
            let mut r = Reader { d: &b, p: 0 };
            assert_eq!(r.varint().unwrap(), v);
            assert_eq!(r.p, b.len());
        }
    }

    #[test]
    fn small_profile_roundtrip() {
        // A real 66-byte player_profile.
        let hex = "ff446654010000000a0000000568696e74730768696e745f6964157475746f7269616c5f64656d6f5f77656c636f6d650573686f776e247475746f7269616c5f74776f5f6163746976655f736b696c6c735f617661696c61626c65187475746f7269616c5f696e7465726163745f6f626a656374177475746f7269616c5f626174746c655f73746172746564187475746f7269616c5f70617274795f6e6565645f7265737408636f756e746572730d666f756e645f73656372657473030000000000040500000001000402000000000103020003010100000001010402000000000103040003010100000001020402000000000103050003010100000001030402000000000103060003010100000001040402000000000103070003010100000000080402000000010401030000000102010100000000090400000000";
        let b: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let d = Doc::parse(&b).unwrap();
        assert_eq!(d.strings.len(), 10);
        assert_eq!(d.to_bytes(), b);
    }

    /// Byte-exact round-trip over every DfT file in a local game install and
    /// the test fixtures. Set UNDERKEEP_DIR to point at the install; skipped
    /// if no install is found.
    #[test]
    fn game_files_roundtrip() {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Ok(d) = std::env::var("UNDERKEEP_DIR") {
            dirs.push(PathBuf::from(&d).join("base"));
            dirs.push(PathBuf::from(&d).join("preload"));
        } else if let Some(game) = crate::paths::find_game_dir() {
            dirs.push(game.join("base"));
            dirs.push(game.join("preload"));
        }
        if let Ok(d) = std::env::var("UNDERKEEP_SAVES") {
            dirs.push(PathBuf::from(d));
        }
        let mut n = 0;
        for dir in dirs {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for f in rd.flatten() {
                let b = std::fs::read(f.path()).unwrap();
                if b.len() < 4 || b[..4] != MAGIC {
                    continue;
                }
                let d = Doc::parse(&b).unwrap_or_else(|e| panic!("{}: {e}", f.path().display()));
                let out = d.to_bytes();
                if out != b {
                    let at = out
                        .iter()
                        .zip(&b)
                        .position(|(x, y)| x != y)
                        .unwrap_or(out.len().min(b.len()));
                    panic!(
                        "{} did not round-trip: first diff at {at:#x} (len {} vs {})",
                        f.path().display(),
                        out.len(),
                        b.len()
                    );
                }
                n += 1;
            }
        }
        eprintln!("round-tripped {n} files");
    }
}
