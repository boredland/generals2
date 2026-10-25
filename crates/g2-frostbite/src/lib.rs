//! Reads the Frostbite 2 data of the 2013 *Command & Conquer* closed alpha from a copy the
//! player owns. Nothing read here is part of this project: the reader only exists so the
//! importer can turn the player's own files into a local cache.
//!
//! Layout (`Data/`): `cas.cat` indexes content-addressed blobs (by SHA-1) inside
//! `cas_NN.cas`; every `Win32/**/*.toc` is a superbundle whose `.sb` holds bundle manifests
//! naming `ebx` (asset), `res` (resource) and chunk entries. Localized chunks live in their
//! own `.sb` at an offset instead of in the CAS. Written from the alpha's bytes, with
//! frankelstner's BF3 dumper notes and mitsuhiko/frostbite2-stuff consulted for the
//! container layout; constants below were checked against the alpha's files.

pub mod audio;
pub mod dbo;
pub mod ebx;
pub mod import;
pub mod material;
pub mod media;
pub mod mesh;
pub mod movie;
pub mod skeleton;
pub mod texture;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use dbo::Value;

#[derive(Debug)]
pub enum Error {
    Io(PathBuf, std::io::Error),
    Truncated,
    Bad(&'static str),
    /// No `cas.cat` under the given folder or its `Data/`.
    NotAnInstall(PathBuf),
    Missing(String),
    Unsupported(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(p, e) => write!(f, "{}: {e}", p.display()),
            Error::Truncated => write!(f, "truncated data"),
            Error::Bad(what) => write!(f, "invalid data: {what}"),
            Error::NotAnInstall(p) => write!(f, "{}: no Data/cas.cat, not an alpha install", p.display()),
            Error::Missing(what) => write!(f, "not found: {what}"),
            Error::Unsupported(what) => write!(f, "unsupported: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// Superbundle (`.toc`) and loose `.sb` files may be obfuscated: a `00 D1 CE 00` magic, a
/// 257-byte key at 0x128, and the payload from 0x22C XORed with `key[i % 257] ^ 0x7B`.
pub fn deobfuscate(bytes: Vec<u8>) -> Result<Vec<u8>, Error> {
    if !bytes.starts_with(&[0x00, 0xd1, 0xce, 0x00]) {
        return Ok(bytes);
    }
    if bytes.len() < 0x22c {
        return Err(Error::Truncated);
    }
    let key = &bytes[0x128..0x128 + 257];
    Ok(bytes[0x22c..].iter().enumerate().map(|(i, b)| b ^ key[i % 257] ^ 0x7b).collect())
}

/// Frostbite payload compression: a run of blocks, each a big-endian decompressed size and
/// a big-endian word holding a type byte and the 24-bit stored size. Equal sizes mean the
/// block is stored; otherwise it is zlib. Returns `None` when `bytes` is not such a run,
/// which is how raw chunks (audio, video) are told apart: nothing else says which is which.
pub fn unblock(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < bytes.len() {
        let head = bytes.get(p..p + 8)?;
        let usize_ = u32::from_be_bytes(head[0..4].try_into().unwrap()) as usize;
        let word = u32::from_be_bytes(head[4..8].try_into().unwrap());
        let (kind, csize) = (word >> 24, (word & 0xff_ffff) as usize);
        // Blocks hold at most 64 KiB; type 0 is the only one this build uses.
        if kind != 0 || usize_ == 0 || usize_ > 0x1_0000 {
            return None;
        }
        p += 8;
        let body = bytes.get(p..p + csize)?;
        if csize == usize_ {
            out.extend_from_slice(body);
        } else {
            let block = miniz_oxide::inflate::decompress_to_vec_zlib(body).ok()?;
            if block.len() != usize_ {
                return None;
            }
            out.extend_from_slice(&block);
        }
        p += csize;
    }
    (!out.is_empty()).then_some(out)
}

#[derive(Debug, Clone, Copy)]
struct CasRef {
    cas: u32,
    offset: u64,
    size: u64,
}

#[derive(Debug, Clone)]
enum ChunkRef {
    Cas([u8; 20]),
    /// Stored in a superbundle's own `.sb` (the localized chunks of `Loc/<lang>.sb`).
    InSb {
        sb: usize,
        offset: u64,
        size: u64,
    },
}

/// A named bundle entry.
#[derive(Debug, Clone)]
pub struct Entry {
    pub sha1: [u8; 20],
    pub size: u64,
    pub original_size: u64,
    /// Res only: Frostbite's hash of the resource type name.
    pub res_type: Option<u32>,
    /// Res only: 16 type-specific bytes (a MeshSet's starts with its header size).
    pub res_meta: Option<[u8; 16]>,
}

/// Resource type hashes this build uses, identified from the names and contents.
pub mod res_type {
    pub const TEXTURE: u32 = 0x5c49_54a6;
    pub const MESH_SET: u32 = 0x49b1_56d4;
}

pub struct Alpha {
    data: PathBuf,
    cas: HashMap<[u8; 20], CasRef>,
    chunks: HashMap<[u8; 16], ChunkRef>,
    sbs: Vec<PathBuf>,
    pub ebx: BTreeMap<String, Entry>,
    pub res: BTreeMap<String, Entry>,
    pub superbundles: usize,
    pub bundles: usize,
}

impl Alpha {
    /// Indexes an install: `path` may be the game folder or its `Data/`.
    pub fn open(path: &Path) -> Result<Alpha, Error> {
        let data = [path.join("Data"), path.to_path_buf()]
            .into_iter()
            .find(|d| d.join("cas.cat").is_file())
            .ok_or_else(|| Error::NotAnInstall(path.to_path_buf()))?;
        let mut a = Alpha {
            cas: read_cat(&read(&data.join("cas.cat"))?)?,
            data,
            chunks: HashMap::new(),
            sbs: Vec::new(),
            ebx: BTreeMap::new(),
            res: BTreeMap::new(),
            superbundles: 0,
            bundles: 0,
        };
        let mut tocs = Vec::new();
        find_tocs(&a.data.join("Win32"), &mut tocs)?;
        tocs.sort();
        for toc in tocs {
            a.add_superbundle(&toc)?;
        }
        Ok(a)
    }

    fn add_superbundle(&mut self, toc_path: &Path) -> Result<(), Error> {
        let toc = dbo::parse(&deobfuscate(read(toc_path)?)?)?;
        let sb_path = toc_path.with_extension("sb");
        let sb_index = self.sbs.len();
        self.sbs.push(sb_path.clone());
        self.superbundles += 1;
        for c in toc.get("chunks").map(Value::items).unwrap_or_default() {
            let id = c.get("id").and_then(Value::guid).ok_or(Error::Bad("chunk without id"))?;
            let r = match c.get("sha1").and_then(Value::sha1) {
                Some(sha1) => ChunkRef::Cas(sha1),
                None => ChunkRef::InSb { sb: sb_index, offset: uint(c, "offset")?, size: uint(c, "size")? },
            };
            self.chunks.entry(id).or_insert(r);
        }
        let bundles = toc.get("bundles").map(Value::items).unwrap_or_default();
        if bundles.is_empty() {
            return Ok(());
        }
        if toc.get("cas").and_then(Value::bool) != Some(true) {
            return Err(Error::Unsupported(format!("{}: bundles stored outside the CAS", toc_path.display())));
        }
        let sb = deobfuscate(read(&sb_path)?)?;
        for b in bundles {
            let (off, size) = (uint(b, "offset")? as usize, uint(b, "size")? as usize);
            let bytes = sb.get(off..off + size).ok_or(Error::Truncated)?;
            let bundle = dbo::parse(bytes)?;
            self.bundles += 1;
            for e in bundle.get("ebx").map(Value::items).unwrap_or_default() {
                let (name, entry) = entry(e, None)?;
                self.ebx.entry(name).or_insert(entry);
            }
            for e in bundle.get("res").map(Value::items).unwrap_or_default() {
                let t = e.get("resType").and_then(Value::int).ok_or(Error::Bad("res without type"))?;
                let (name, entry) = entry(e, Some(t as u32))?;
                self.res.entry(name).or_insert(entry);
            }
            for c in bundle.get("chunks").map(Value::items).unwrap_or_default() {
                let id = c.get("id").and_then(Value::guid).ok_or(Error::Bad("chunk without id"))?;
                if let Some(sha1) = c.get("sha1").and_then(Value::sha1) {
                    self.chunks.entry(id).or_insert(ChunkRef::Cas(sha1));
                }
            }
        }
        Ok(())
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// The game folder: the parent of `Data/` (loose files such as `UTFWinAssets/` live
    /// there).
    pub fn root(&self) -> &Path {
        self.data.parent().unwrap_or(&self.data)
    }

    /// The stored bytes of a CAS blob, as they are on disk (possibly block-compressed).
    pub fn cas_blob(&self, sha1: &[u8; 20]) -> Result<Vec<u8>, Error> {
        let r = self.cas.get(sha1).ok_or_else(|| Error::Missing(format!("CAS blob {}", hex(sha1))))?;
        read_at(&self.data.join(format!("cas_{:02}.cas", r.cas)), r.offset, r.size)
    }

    /// An ebx or res entry's payload, decompressed.
    pub fn entry_bytes(&self, e: &Entry) -> Result<Vec<u8>, Error> {
        let out = unblock(&self.cas_blob(&e.sha1)?).ok_or(Error::Bad("entry is not block-compressed"))?;
        if out.len() as u64 != e.original_size {
            return Err(Error::Bad("entry decompressed to the wrong size"));
        }
        Ok(out)
    }

    /// An ebx asset, parsed.
    pub fn asset(&self, name: &str) -> Result<ebx::Partition, Error> {
        let e = self.ebx.get(name).ok_or_else(|| Error::Missing(format!("ebx {name}")))?;
        ebx::parse(&self.entry_bytes(e)?)
    }

    /// A chunk as stored.
    pub fn chunk_raw(&self, id: &[u8; 16]) -> Result<Vec<u8>, Error> {
        match self.chunks.get(id).ok_or_else(|| Error::Missing(format!("chunk {}", hex(id))))? {
            ChunkRef::Cas(sha1) => self.cas_blob(sha1),
            ChunkRef::InSb { sb, offset, size } => read_at(&self.sbs[*sb], *offset, *size),
        }
    }

    /// A chunk's payload: decompressed when it is block-compressed, else as stored (the
    /// audio and video chunks; see the install test for what that covers).
    pub fn chunk(&self, id: &[u8; 16]) -> Result<Vec<u8>, Error> {
        let raw = self.chunk_raw(id)?;
        Ok(unblock(&raw).unwrap_or(raw))
    }

    pub fn has_chunk(&self, id: &[u8; 16]) -> bool {
        self.chunks.contains_key(id)
    }

    pub fn chunk_ids(&self) -> impl Iterator<Item = &[u8; 16]> {
        self.chunks.keys()
    }

    /// Res entries of one type, by name.
    pub fn res_of(&self, t: u32) -> impl Iterator<Item = (&String, &Entry)> {
        self.res.iter().filter(move |(_, e)| e.res_type == Some(t))
    }
}

fn entry(e: &Value, res_type: Option<u32>) -> Result<(String, Entry), Error> {
    let name = e.get("name").and_then(Value::str).ok_or(Error::Bad("entry without name"))?;
    let sha1 = e.get("sha1").and_then(Value::sha1).ok_or(Error::Bad("entry without sha1"))?;
    let res_meta = match e.get("resMeta") {
        Some(Value::Blob(b)) => Some(b.as_slice().try_into().map_err(|_| Error::Bad("resMeta is not 16 bytes"))?),
        _ => None,
    };
    Ok((
        name.to_owned(),
        Entry { sha1, size: uint(e, "size")?, original_size: uint(e, "originalSize")?, res_type, res_meta },
    ))
}

fn uint(v: &Value, key: &'static str) -> Result<u64, Error> {
    v.get(key).and_then(Value::int).and_then(|i| u64::try_from(i).ok()).ok_or(Error::Bad(key))
}

/// `cas.cat`: "NyanNyanNyanNyan", then 32-byte records of SHA-1, offset, size and CAS
/// file number (little-endian `u32`s). The offset points past each blob's own header.
fn read_cat(bytes: &[u8]) -> Result<HashMap<[u8; 20], CasRef>, Error> {
    let body = bytes.strip_prefix(b"NyanNyanNyanNyan").ok_or(Error::Bad("cas.cat magic"))?;
    let (records, rest) = body.as_chunks::<32>();
    if !rest.is_empty() {
        return Err(Error::Truncated);
    }
    Ok(records
        .iter()
        .map(|r| {
            let u = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
            (r[..20].try_into().unwrap(), CasRef { offset: u(20).into(), size: u(24).into(), cas: u(28) })
        })
        .collect())
}

fn find_tocs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Error> {
    for e in std::fs::read_dir(dir).map_err(|e| Error::Io(dir.to_path_buf(), e))? {
        let path = e.map_err(|e| Error::Io(dir.to_path_buf(), e))?.path();
        if path.is_dir() {
            find_tocs(&path, out)?;
        } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("toc")) {
            out.push(path);
        }
    }
    Ok(())
}

fn read(path: &Path) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|e| Error::Io(path.to_path_buf(), e))
}

fn read_at(path: &Path, offset: u64, size: u64) -> Result<Vec<u8>, Error> {
    let io = |e| Error::Io(path.to_path_buf(), e);
    let mut f = File::open(path).map_err(io)?;
    f.seek(SeekFrom::Start(offset)).map_err(io)?;
    let mut buf = vec![0; size as usize];
    f.read_exact(&mut buf).map_err(io)?;
    Ok(buf)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deobfuscation_uses_the_257_byte_key_from_0x128() {
        let mut file = vec![0u8; 0x22c];
        file[..4].copy_from_slice(&[0x00, 0xd1, 0xce, 0x00]);
        for (i, k) in file[0x128..0x128 + 257].iter_mut().enumerate() {
            *k = i as u8 ^ 0x5a;
        }
        let plain: Vec<u8> = (0..600u32).map(|i| (i * 7) as u8).collect();
        let key = file[0x128..0x128 + 257].to_vec();
        file.extend(plain.iter().enumerate().map(|(i, b)| b ^ key[i % 257] ^ 0x7b));
        assert_eq!(deobfuscate(file).unwrap(), plain);
        // Plain files pass through untouched.
        assert_eq!(deobfuscate(vec![1, 2, 3]).unwrap(), vec![1, 2, 3]);
    }

    fn block(data: &[u8], compress: bool) -> Vec<u8> {
        let body = if compress { miniz_oxide::deflate::compress_to_vec_zlib(data, 6) } else { data.to_vec() };
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend((body.len() as u32).to_be_bytes());
        out.extend(body);
        out
    }

    #[test]
    fn block_runs_mix_stored_and_zlib_blocks() {
        let a: Vec<u8> = (0..0x1_0000u32).map(|i| (i / 300) as u8).collect();
        let b = b"tail block, stored".to_vec();
        let mut run = block(&a, true);
        run.extend(block(&b, false));
        assert_eq!(unblock(&run).unwrap(), [a, b].concat());
    }

    #[test]
    fn raw_audio_and_truncated_runs_are_not_block_runs() {
        // An EA SNR header (0x48 block, 48 kHz): the size word is far above 64 KiB.
        let snr = [0x48, 0x00, 0x00, 0x0c, 0x14, 0x04, 0xbb, 0x80, 0x40, 0x80, 0x54, 0x79];
        assert_eq!(unblock(&snr), None);
        let mut run = block(b"0123456789", false);
        run.pop();
        assert_eq!(unblock(&run), None);
    }

    #[test]
    fn cat_records_map_sha1_to_cas_offset_and_size() {
        let mut cat = b"NyanNyanNyanNyan".to_vec();
        let sha = [7u8; 20];
        cat.extend(sha);
        for v in [0x30u32, 0x1234, 2] {
            cat.extend(v.to_le_bytes());
        }
        let map = read_cat(&cat).unwrap();
        let r = map[&sha];
        assert_eq!((r.offset, r.size, r.cas), (0x30, 0x1234, 2));
        cat.push(0);
        assert!(read_cat(&cat).is_err());
    }
}
