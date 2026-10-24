//! Frostbite's binary JSON ("DbObject"), the encoding of every `.toc` and of the bundle
//! headers inside `.sb` files. Each value is a type byte (low 5 bits; the high 3 are flags
//! nobody has decoded and the reader ignores), then the payload. Lists and dicts carry a size
//! varint but are actually terminated by a zero type byte.

use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    List(Vec<Value>),
    /// In file order; keys are unique in practice, and lookup takes the first.
    Dict(Vec<(String, Value)>),
    /// Type 5: eight bytes of unknown meaning.
    Opaque([u8; 8]),
    Bool(bool),
    String(String),
    Int(i64),
    Guid([u8; 16]),
    Sha1([u8; 20]),
    Blob(Vec<u8>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(d) => d.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The list's items, or none for anything else (absent lists are common).
    pub fn items(&self) -> &[Value] {
        match self {
            Value::List(l) => l,
            _ => &[],
        }
    }

    pub fn int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn guid(&self) -> Option<[u8; 16]> {
        match self {
            Value::Guid(g) => Some(*g),
            _ => None,
        }
    }

    pub fn sha1(&self) -> Option<[u8; 20]> {
        match self {
            Value::Sha1(s) => Some(*s),
            _ => None,
        }
    }
}

pub fn parse(bytes: &[u8]) -> Result<Value, Error> {
    let mut r = Reader { b: bytes, p: 0 };
    let t = r.u8()?;
    r.value(t, 0)
}

/// Nesting in real files is a handful of levels; the cap only stops corrupt input from
/// overflowing the stack.
const MAX_DEPTH: u32 = 64;

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.p.checked_add(n).filter(|&e| e <= self.b.len()).ok_or(Error::Truncated)?;
        let out = &self.b[self.p..end];
        self.p = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn varint(&mut self) -> Result<usize, Error> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.u8()?;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return usize::try_from(v).map_err(|_| Error::Bad("varint too large"));
            }
        }
        Err(Error::Bad("varint too long"))
    }

    fn cstring(&mut self) -> Result<String, Error> {
        let n = self.b[self.p..].iter().position(|&c| c == 0).ok_or(Error::Truncated)?;
        let s = String::from_utf8_lossy(self.take(n)?).into_owned();
        self.p += 1;
        Ok(s)
    }

    fn value(&mut self, type_byte: u8, depth: u32) -> Result<Value, Error> {
        if depth > MAX_DEPTH {
            return Err(Error::Bad("DbObject nested too deeply"));
        }
        Ok(match type_byte & 0x1f {
            0 => Value::Null,
            1 => {
                self.varint()?;
                let mut items = Vec::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    items.push(self.value(t, depth + 1)?);
                }
                Value::List(items)
            }
            2 => {
                self.varint()?;
                let mut fields = Vec::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let key = self.cstring()?;
                    fields.push((key, self.value(t, depth + 1)?));
                }
                Value::Dict(fields)
            }
            5 => Value::Opaque(self.take(8)?.try_into().unwrap()),
            6 => Value::Bool(self.u8()? != 0),
            7 => {
                // The length counts the terminating NUL.
                let n = self.varint()?;
                let raw = self.take(n)?;
                let raw = raw.strip_suffix(&[0]).ok_or(Error::Bad("string without NUL"))?;
                Value::String(String::from_utf8_lossy(raw).into_owned())
            }
            8 => Value::Int(i32::from_le_bytes(self.take(4)?.try_into().unwrap()).into()),
            9 => Value::Int(i64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            15 => Value::Guid(self.take(16)?.try_into().unwrap()),
            16 => Value::Sha1(self.take(20)?.try_into().unwrap()),
            19 => {
                let n = self.varint()?;
                Value::Blob(self.take(n)?.to_vec())
            }
            _ => return Err(Error::Bad("unknown DbObject type")),
        })
    }
}
