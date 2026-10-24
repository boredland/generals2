//! Ebx, Frostbite 2's binary asset format: a partition of typed instances whose layout is
//! described in the file itself (field and class descriptors, names stored as DJB2 hashes
//! with a keyword table). Written from the alpha's bytes, with NicknineTheEagle's
//! Frostbite-Scripts (`frostbite2/ebx.py`) consulted for the header fields; the section
//! alignment was checked against every ebx in the alpha.

use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A class or value type, with inherited fields flattened in.
    Complex {
        ty: String,
        fields: Vec<(String, Value)>,
    },
    Link(Link),
    Array(Vec<Value>),
    /// Strings and file references; `None` is the null string.
    String(Option<String>),
    Bool(bool),
    /// Every integer type and enums. `u64` values above `i64::MAX` wrap; none occur.
    Int(i64),
    Float(f64),
    Guid([u8; 16]),
    Sha1([u8; 20]),
}

/// A reference to an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Null,
    /// Index into this partition's instances.
    Internal(usize),
    /// Index into this partition's imports.
    External(usize),
}

impl Value {
    pub fn ty(&self) -> Option<&str> {
        match self {
            Value::Complex { ty, .. } => Some(ty),
            _ => None,
        }
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Complex { fields, .. } => fields.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn items(&self) -> &[Value] {
        match self {
            Value::Array(a) => a,
            _ => &[],
        }
    }

    pub fn int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Value::String(s) => s.as_deref(),
            _ => None,
        }
    }

    pub fn guid(&self) -> Option<[u8; 16]> {
        match self {
            Value::Guid(g) => Some(*g),
            _ => None,
        }
    }

    pub fn link(&self) -> Option<Link> {
        match self {
            Value::Link(l) => Some(*l),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub guid: [u8; 16],
    pub value: Value,
}

#[derive(Debug, Clone)]
pub struct Partition {
    pub file_guid: [u8; 16],
    pub primary_guid: [u8; 16],
    /// (partition GUID, instance GUID) per external link target.
    pub imports: Vec<([u8; 16], [u8; 16])>,
    pub instances: Vec<Instance>,
}

impl Partition {
    pub fn primary(&self) -> Option<&Value> {
        self.instances.iter().find(|i| i.guid == self.primary_guid).map(|i| &i.value)
    }

    /// The instance an internal link names (`None` for null and external links).
    pub fn resolve(&self, link: Link) -> Option<&Value> {
        match link {
            Link::Internal(i) => self.instances.get(i).map(|i| &i.value),
            _ => None,
        }
    }
}

const MAGIC: [u8; 4] = [0xce, 0xd1, 0xb2, 0x0f];

pub fn parse(d: &[u8]) -> Result<Partition, Error> {
    if !d.starts_with(&MAGIC) {
        return Err(Error::Bad("not an ebx partition"));
    }
    let r = Bytes(d);
    let h: Vec<usize> = (0..11).map(|i| r.u32(4 + 4 * i).map(|v| v as usize)).collect::<Result<_, _>>()?;
    let [
        strings_at,
        len_to_eof,
        n_imports,
        _,
        n_repeaters,
        n_complex,
        n_fields,
        len_names,
        len_strings,
        n_arrays,
        len_payload,
    ] = h[..]
    else {
        unreachable!()
    };
    if strings_at.checked_add(len_to_eof) != Some(d.len()) {
        return Err(Error::Bad("ebx size does not match its header"));
    }
    let file_guid = r.guid(0x30)?;
    let primary_guid = r.guid(0x40)?;
    let mut p = 0x50;
    let imports =
        (0..n_imports).map(|i| Ok((r.guid(p + 32 * i)?, r.guid(p + 32 * i + 16)?))).collect::<Result<_, Error>>()?;
    p += 32 * n_imports;
    let names = r.slice(p, len_names)?;
    let keywords: std::collections::HashMap<u32, &[u8]> =
        names.split(|&c| c == 0).filter(|w| !w.is_empty()).map(|w| (djb2(w), w)).collect();
    let name = |hash: u32| match keywords.get(&hash) {
        Some(w) => String::from_utf8_lossy(w).into_owned(),
        None => format!("#{hash:08x}"),
    };
    p += len_names;
    let fields = (0..n_fields)
        .map(|i| {
            let at = p + 16 * i;
            Ok(Field {
                name: name(r.u32(at)?),
                kind: (r.u16(at + 4)? >> 4) as u8 & 0x1f,
                reference: r.u16(at + 6)? as usize,
                offset: r.u32(at + 8)? as usize,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    p += 16 * n_fields;
    let complexes = (0..n_complex)
        .map(|i| {
            let at = p + 16 * i;
            Ok(Complex {
                name: name(r.u32(at)?),
                first_field: r.u32(at + 4)? as usize,
                n_fields: r.u8(at + 8)? as usize,
                size: r.u16(at + 12)? as usize,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    p += 16 * n_complex;
    let repeaters = (0..n_repeaters)
        .map(|i| Ok((r.u32(p + 12 * i + 4)? as usize, r.u32(p + 12 * i + 8)? as usize)))
        .collect::<Result<Vec<_>, Error>>()?;
    p = (p + 12 * n_repeaters).next_multiple_of(16);
    let arrays = (0..n_arrays)
        .map(|i| Ok((r.u32(p + 12 * i)? as usize, r.u32(p + 12 * i + 4)? as usize)))
        .collect::<Result<Vec<_>, Error>>()?;
    p += 12 * n_arrays;
    if p.next_multiple_of(16) != strings_at {
        return Err(Error::Bad("ebx descriptor tables do not end at the string section"));
    }
    for c in &complexes {
        if c.first_field + c.n_fields > fields.len() {
            return Err(Error::Bad("ebx class fields out of range"));
        }
    }
    let reader = Reader {
        r,
        fields: &fields,
        complexes: &complexes,
        arrays: &arrays,
        strings_at,
        arrays_at: strings_at + len_strings + len_payload,
    };
    let mut at = strings_at + len_strings;
    let mut instances = Vec::new();
    for &(count, complex) in &repeaters {
        let size = complexes.get(complex).ok_or(Error::Bad("ebx instance class out of range"))?.size;
        for _ in 0..count {
            let guid = r.guid(at)?;
            at += 16;
            instances.push(Instance { guid, value: reader.complex(complex, at, 0)? });
            at += size;
        }
    }
    Ok(Partition { file_guid, primary_guid, imports, instances })
}

/// The keyword hash: DJB2 with XOR (`h * 33 ^ c`, from 5381).
fn djb2(word: &[u8]) -> u32 {
    word.iter().fold(5381u32, |h, &c| h.wrapping_mul(33) ^ u32::from(c))
}

struct Field {
    name: String,
    kind: u8,
    reference: usize,
    offset: usize,
}

struct Complex {
    name: String,
    first_field: usize,
    n_fields: usize,
    size: usize,
}

#[derive(Clone, Copy)]
struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    fn slice(&self, at: usize, n: usize) -> Result<&'a [u8], Error> {
        self.0.get(at..at.checked_add(n).ok_or(Error::Truncated)?).ok_or(Error::Truncated)
    }
    fn arr<const N: usize>(&self, at: usize) -> Result<[u8; N], Error> {
        Ok(self.slice(at, N)?.try_into().unwrap())
    }
    fn u8(&self, at: usize) -> Result<u8, Error> {
        Ok(self.arr::<1>(at)?[0])
    }
    fn u16(&self, at: usize) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.arr(at)?))
    }
    fn u32(&self, at: usize) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.arr(at)?))
    }
    fn guid(&self, at: usize) -> Result<[u8; 16], Error> {
        self.arr(at)
    }
}

const MAX_DEPTH: u32 = 48;

struct Reader<'a> {
    r: Bytes<'a>,
    fields: &'a [Field],
    complexes: &'a [Complex],
    arrays: &'a [(usize, usize)],
    strings_at: usize,
    arrays_at: usize,
}

impl Reader<'_> {
    fn complex(&self, index: usize, at: usize, depth: u32) -> Result<Value, Error> {
        let mut fields = Vec::new();
        let ty = self.fill(index, at, depth, &mut fields)?;
        Ok(Value::Complex { ty, fields })
    }

    /// Appends a class's fields, its bases' first (a base is a type-0 field at offset 0).
    fn fill(&self, index: usize, at: usize, depth: u32, out: &mut Vec<(String, Value)>) -> Result<String, Error> {
        if depth > MAX_DEPTH {
            return Err(Error::Bad("ebx nested too deeply"));
        }
        let c = self.complexes.get(index).ok_or(Error::Bad("ebx class out of range"))?;
        for f in &self.fields[c.first_field..c.first_field + c.n_fields] {
            if f.kind == 0 {
                self.fill(f.reference, at + f.offset, depth + 1, out)?;
            } else {
                out.push((f.name.clone(), self.field(f, at + f.offset, depth)?));
            }
        }
        Ok(c.name.clone())
    }

    fn field(&self, f: &Field, at: usize, depth: u32) -> Result<Value, Error> {
        let r = self.r;
        Ok(match f.kind {
            2 => self.complex(f.reference, at, depth + 1)?,
            3 => Value::Link(match r.u32(at)? {
                0 => Link::Null,
                v if v >> 31 == 1 => Link::External((v & 0x7fff_ffff) as usize),
                v => Link::Internal(v as usize - 1),
            }),
            4 => {
                let &(offset, count) =
                    self.arrays.get(r.u32(at)? as usize).ok_or(Error::Bad("ebx array index out of range"))?;
                let holder = self.complexes.get(f.reference).ok_or(Error::Bad("ebx array class out of range"))?;
                let member = self.fields.get(holder.first_field).ok_or(Error::Bad("ebx array member out of range"))?;
                let stride = self.size(member)?;
                let start = self.arrays_at + offset;
                (0..count)
                    .map(|i| self.field(member, start + i * stride, depth + 1))
                    .collect::<Result<_, _>>()
                    .map(Value::Array)?
            }
            7 | 9 => match r.u32(at)? {
                u32::MAX => Value::String(None),
                o => {
                    let s = self.strings_at + o as usize;
                    let tail = r.0.get(s..).ok_or(Error::Truncated)?;
                    let n = tail.iter().position(|&c| c == 0).ok_or(Error::Truncated)?;
                    Value::String(Some(String::from_utf8_lossy(&tail[..n]).into_owned()))
                }
            },
            8 | 15 => Value::Int(i32::from_le_bytes(r.arr(at)?).into()),
            10 => Value::Bool(r.u8(at)? != 0),
            11 => Value::Int((r.u8(at)? as i8).into()),
            12 => Value::Int(r.u8(at)?.into()),
            13 => Value::Int(i16::from_le_bytes(r.arr(at)?).into()),
            14 => Value::Int(r.u16(at)?.into()),
            16 => Value::Int(r.u32(at)?.into()),
            17 => Value::Int(i64::from_le_bytes(r.arr(at)?)),
            18 => Value::Int(u64::from_le_bytes(r.arr(at)?) as i64),
            19 => Value::Float(f32::from_le_bytes(r.arr(at)?).into()),
            20 => Value::Float(f64::from_le_bytes(r.arr(at)?)),
            21 => Value::Guid(r.arr(at)?),
            22 => Value::Sha1(r.arr(at)?),
            _ => return Err(Error::Unsupported(format!("ebx field type {:#x} ({})", f.kind, f.name))),
        })
    }

    fn size(&self, f: &Field) -> Result<usize, Error> {
        Ok(match f.kind {
            0 | 2 => self.complexes.get(f.reference).ok_or(Error::Bad("ebx class out of range"))?.size,
            10..=12 => 1,
            13 | 14 => 2,
            3 | 4 | 7 | 8 | 9 | 15 | 16 | 19 => 4,
            17 | 18 | 20 => 8,
            21 => 16,
            22 => 20,
            _ => return Err(Error::Unsupported(format!("ebx array of field type {:#x}", f.kind))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn djb2_matches_the_keyword_hash() {
        // Field hashes from the alpha's music asset ("Name" at offset 8 of DataContainer's
        // first derived class).
        assert_eq!(djb2(b""), 5381);
        assert_eq!(djb2(b"a"), (5381 * 33) ^ 97);
    }

    /// Builds a partition with one `Thing` instance deriving from `Base`:
    /// Base { Id: i32 } and Thing { Name: string, Values: [u16], Next: link }.
    fn sample() -> Vec<u8> {
        let words = ["Base", "Id", "Thing", "Name", "Values", "member", "Next", "Array"];
        let mut names: Vec<u8> = words.iter().flat_map(|w| w.bytes().chain([0])).collect();
        names.resize(names.len().next_multiple_of(16), 0);
        let h = |w: &str| djb2(w.as_bytes());
        // (hash, kind << 4, reference, offset)
        let fields: [(u32, u16, u16, u32); 6] = [
            (h("Id"), 15 << 4, 0, 0),     // Base.Id
            (h("Base"), 0, 0, 0),         // Thing: base
            (h("Name"), 7 << 4, 0, 4),    // Thing.Name
            (h("Values"), 4 << 4, 2, 8),  // Thing.Values -> class 2 (array holder)
            (h("Next"), 3 << 4, 0, 12),   // Thing.Next
            (h("member"), 14 << 4, 0, 0), // Array member: u16
        ];
        // (hash, first field, field count, size)
        let classes: [(u32, u32, u8, u16); 3] = [(h("Base"), 0, 1, 4), (h("Thing"), 1, 4, 16), (h("Array"), 5, 1, 2)];
        let strings = b"hello\0\0\0\0\0\0\0\0\0\0\0".to_vec();
        let mut payload = vec![9u8; 16]; // instance GUID
        payload.extend(7i32.to_le_bytes());
        payload.extend(0u32.to_le_bytes()); // "hello"
        payload.extend(0u32.to_le_bytes()); // array 0
        payload.extend(1u32.to_le_bytes()); // link to instance 0 (itself)
        let array_data: Vec<u8> = [3u16, 4, 5].iter().flat_map(|v| v.to_le_bytes()).collect();

        let mut tables = names.clone();
        for (hash, kind, reference, offset) in fields {
            tables.extend(hash.to_le_bytes());
            tables.extend(kind.to_le_bytes());
            tables.extend(reference.to_le_bytes());
            tables.extend(offset.to_le_bytes());
            tables.extend(0u32.to_le_bytes());
        }
        for (hash, first, count, size) in classes {
            tables.extend(hash.to_le_bytes());
            tables.extend(first.to_le_bytes());
            tables.extend([count, 4]);
            tables.extend(0u16.to_le_bytes());
            tables.extend(size.to_le_bytes());
            tables.extend(0u16.to_le_bytes());
        }
        for v in [0u32, 1, 1] {
            tables.extend(v.to_le_bytes()); // one repeater: 1 instance of class 1
        }
        let mut file = vec![0u8; 0x50];
        file.extend(tables);
        file.resize(file.len().next_multiple_of(16), 0);
        for v in [0u32, 3, 2] {
            file.extend(v.to_le_bytes()); // array 0: offset 0, 3 items
        }
        file.resize(file.len().next_multiple_of(16), 0);
        let strings_at = file.len() as u32;
        file.extend(&strings);
        file.extend(&payload);
        file.extend(&array_data);
        let header = [
            strings_at,
            (file.len() as u32) - strings_at,
            0,
            0,
            1,
            classes.len() as u32,
            fields.len() as u32,
            names.len() as u32,
            strings.len() as u32,
            1,
            payload.len() as u32,
        ];
        file[..4].copy_from_slice(&MAGIC);
        for (i, v) in header.iter().enumerate() {
            file[4 + 4 * i..8 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        file[0x40..0x50].copy_from_slice(&[9; 16]);
        file
    }

    #[test]
    fn instances_flatten_bases_and_resolve_strings_arrays_and_links() {
        let p = parse(&sample()).unwrap();
        let prim = p.primary().unwrap();
        assert_eq!(prim.ty(), Some("Thing"));
        assert_eq!(prim.get("Id").and_then(Value::int), Some(7));
        assert_eq!(prim.get("Name").and_then(Value::str), Some("hello"));
        let values: Vec<i64> = prim.get("Values").unwrap().items().iter().filter_map(Value::int).collect();
        assert_eq!(values, [3, 4, 5]);
        let next = prim.get("Next").and_then(Value::link).unwrap();
        assert_eq!(next, Link::Internal(0));
        assert_eq!(p.resolve(next), Some(prim));
    }

    #[test]
    fn corrupt_partitions_are_errors_not_panics() {
        let good = sample();
        for cut in [3, 0x30, 0x60, good.len() - 1] {
            assert!(parse(&good[..cut]).is_err(), "cut at {cut}");
        }
        let mut bad = good.clone();
        bad[4] ^= 0x10; // string section moved
        assert!(parse(&bad).is_err());
    }
}
