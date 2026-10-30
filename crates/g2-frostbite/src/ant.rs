//! The alpha's animation clips: one ANT package (`_rts/art/animation/masteranimations/
//! s_cnco_common`, a res of type `ANT_PACKAGE`) holds every clip, rig and controller.
//!
//! The package is EA's GenericData: a big-endian packaging type and header size, then
//! sections tagged `GD.STRM`, `GD.REFL` (the class layouts, described in the file itself) and
//! one `GD.DATA` per object. Objects reference each other by 16-byte GUIDs. Layout read from
//! the alpha's bytes, with marv7000/AssetBankPlugin (GPL-3.0) and the openBF3 project's ANT
//! notes consulted for the section and layout headers; no code was taken.
//!
//! 1,683 of the 1,700 clips are `VbrAnimationAsset`s (codec "VBR "), the rest raw floats.
//! The VBR bitstream follows two independent GPL-3.0 decoders, which agree:
//! 4FingerStudios/FrostyBlender's `tools/ant.py` and the FIFA 17 Frosty fork's
//! `FrostyAnimationEditor/Core/Anim/VbrClip.cs`. Checked on every clip in the alpha: each
//! frame block decodes using exactly the bits it holds, animated quaternions come out unit
//! length (99% within 0.4%), and each channel lands on a DOF of its own kind.
//!
//! A clip's channels are quaternions, then vectors, then floats; `ChannelToDofAsset` maps
//! channel `c` to DOF `IndexData[c]` of its controller's target hierarchy, whose
//! `LayoutAsset`s list the DOFs in order (`<Joint>.q`, `.t`, `.s`; a `DeltaTrajLayoutAsset`
//! takes 8 unnamed DOFs).

use std::collections::BTreeMap;

use crate::{Alpha, Error};

/// The res type of an ANT package.
pub const ANT_PACKAGE: u32 = 0x51a3_c853;

/// The package every clip of this build lives in.
pub const PACKAGE: &str = "_rts/art/animation/masteranimations/s_cnco_common";

/// Layout slot types, as the `LayoutAsset`s give them.
const ROTATION: i64 = 14;
const TRANSLATION: i64 = 0x7a2e_5497;
const SCALE: i64 = 0x7a2e_53c6;

/// A field value. Byte and float arrays stay packed: clip data runs to megabytes.
#[derive(Debug, Clone, PartialEq)]
pub enum Gd {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Guid([u8; 16]),
    Bytes(Vec<u8>),
    Floats(Vec<f32>),
    Ints(Vec<i64>),
    List(Vec<Gd>),
    Struct(Vec<(String, Gd)>),
}

impl Gd {
    pub fn get(&self, name: &str) -> Option<&Gd> {
        match self {
            Gd::Struct(f) => f.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn int(&self) -> Option<i64> {
        match self {
            Gd::Int(i) => Some(*i),
            Gd::Bool(b) => Some(i64::from(*b)),
            _ => None,
        }
    }
    pub fn float(&self) -> Option<f64> {
        match self {
            Gd::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Gd::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn guid(&self) -> Option<[u8; 16]> {
        match self {
            Gd::Guid(g) => Some(*g),
            _ => None,
        }
    }
    pub fn items(&self) -> &[Gd] {
        match self {
            Gd::List(l) => l,
            _ => &[],
        }
    }
}

/// One object: its class, `__name`, `__guid` and fields (base class fields included).
#[derive(Debug, Clone)]
pub struct Object {
    pub ty: String,
    pub name: String,
    pub fields: Gd,
}

impl Object {
    pub fn get(&self, name: &str) -> Option<&Gd> {
        self.fields.get(name)
    }
    fn int(&self, name: &str) -> Result<i64, Error> {
        self.get(name).and_then(Gd::int).ok_or(Error::Bad("ANT object without an integer field"))
    }
    fn float(&self, name: &str) -> Result<f32, Error> {
        Ok(self.get(name).and_then(Gd::float).ok_or(Error::Bad("ANT object without a float field"))? as f32)
    }
    fn guid(&self, name: &str) -> Option<[u8; 16]> {
        self.get(name).and_then(Gd::guid).filter(|g| *g != [0; 16])
    }
}

/// Every object of a package, by GUID.
pub struct Package {
    pub objects: BTreeMap<[u8; 16], Object>,
}

#[derive(Clone)]
struct Field {
    name: String,
    offset: usize,
    array: bool,
    ty: u32,
}

#[derive(Clone)]
struct Class {
    name: String,
    size: usize,
    align: usize,
    fields: Vec<Field>,
}

/// Little-endian reads that fail instead of panicking.
#[derive(Clone, Copy)]
struct Le<'a>(&'a [u8]);

impl<'a> Le<'a> {
    fn slice(&self, at: usize, n: usize) -> Result<&'a [u8], Error> {
        self.0.get(at..at.checked_add(n).ok_or(Error::Truncated)?).ok_or(Error::Truncated)
    }
    fn arr<const N: usize>(&self, at: usize) -> Result<[u8; N], Error> {
        Ok(self.slice(at, N)?.try_into().unwrap())
    }
    fn u16(&self, at: usize) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.arr(at)?))
    }
    fn u32(&self, at: usize) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.arr(at)?))
    }
    fn i32(&self, at: usize) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.arr(at)?))
    }
    fn i64(&self, at: usize) -> Result<i64, Error> {
        Ok(i64::from_le_bytes(self.arr(at)?))
    }
    fn cstr(&self, at: usize) -> Result<String, Error> {
        let tail = self.0.get(at..).ok_or(Error::Truncated)?;
        let n = tail.iter().position(|&c| c == 0).ok_or(Error::Truncated)?;
        Ok(String::from_utf8_lossy(&tail[..n]).into_owned())
    }
    /// An offset from the file, made an index; negative ones are invalid.
    fn at(&self, base: usize, off: i64) -> Result<usize, Error> {
        usize::try_from(off).ok().and_then(|o| base.checked_add(o)).ok_or(Error::Bad("negative GenericData offset"))
    }
}

impl Package {
    /// The alpha's package, read and parsed.
    pub fn read(alpha: &Alpha) -> Result<Package, Error> {
        let e = alpha.res.get(PACKAGE).ok_or_else(|| Error::Missing(format!("res {PACKAGE}")))?;
        if e.res_type != Some(ANT_PACKAGE) {
            return Err(Error::Bad("animation package is not an ANT package res"));
        }
        Package::parse(&alpha.entry_bytes(e)?)
    }

    pub fn parse(bytes: &[u8]) -> Result<Package, Error> {
        let r = Le(bytes);
        // Big-endian packaging type and header size, then the sections.
        let header = u32::from_be_bytes(r.arr(4)?) as usize;
        let mut at = 4usize.checked_add(header).ok_or(Error::Truncated)?;
        let mut classes: BTreeMap<u32, Class> = BTreeMap::new();
        let mut objects = BTreeMap::new();
        while at < bytes.len() {
            let tag = r.slice(at, 8)?;
            let size = r.u32(at + 8)? as usize;
            match tag {
                b"GD.STRMl" => at += 16,
                b"GD.REFLl" => {
                    classes = reflection(r, at + 16)?;
                    at += size.max(16);
                }
                b"GD.DATAl" => {
                    let o = object(r, at + 16, &classes)?;
                    let guid = o.guid("__guid").ok_or(Error::Bad("ANT object without a GUID"))?;
                    objects.insert(guid, o);
                    at += size.max(16);
                }
                b"GD.STRMb" | b"GD.REFLb" | b"GD.DATAb" => {
                    return Err(Error::Unsupported("big-endian GenericData".into()));
                }
                _ => return Err(Error::Bad("unknown GenericData section")),
            }
        }
        Ok(Package { objects })
    }

    pub fn get(&self, guid: Option<[u8; 16]>) -> Option<&Object> {
        guid.and_then(|g| self.objects.get(&g))
    }

    /// Every rig: its ANT skeleton's joints with their parents, and the clip controllers that
    /// play on it (a controller's target hierarchy lies in the rig's tree of DOF sets).
    pub fn rigs(&self) -> Vec<Rig> {
        let controllers: Vec<(&[u8; 16], &Object)> =
            self.objects.iter().filter(|(_, o)| o.ty == "ClipControllerAsset").collect();
        self.objects
            .values()
            .filter(|o| o.ty == "RigAsset")
            .map(|rig| {
                let joints: Vec<&Gd> = self
                    .get(rig.guid("Skeleton"))
                    .and_then(|s| s.get("Joints"))
                    .map(Gd::items)
                    .unwrap_or_default()
                    .iter()
                    .collect();
                let name = |j: &Gd| j.get("JointName").and_then(Gd::str).unwrap_or_default().to_owned();
                let joints = joints
                    .iter()
                    .map(|j| {
                        let parent = j.get("ParentIndex").and_then(Gd::int).and_then(|p| usize::try_from(p).ok());
                        (name(j), parent.and_then(|p| joints.get(p)).map(|p| name(p)))
                    })
                    .collect();
                let mut tree = Vec::new();
                for root in rig.get("DofSetLists").and_then(guids).unwrap_or_default() {
                    self.hierarchy_tree(root, &mut tree);
                }
                let clips = controllers
                    .iter()
                    .filter(|(_, c)| c.guid("Target").is_some_and(|t| tree.contains(&t)))
                    .map(|(g, _)| **g)
                    .collect();
                Rig { name: rig.name.clone(), joints, clips }
            })
            .collect()
    }

    fn hierarchy_tree(&self, at: [u8; 16], out: &mut Vec<[u8; 16]>) {
        let Some(h) = self.objects.get(&at).filter(|o| o.ty == "LayoutHierarchyAsset") else { return };
        if out.contains(&at) {
            return;
        }
        out.push(at);
        for c in h.get("Children").and_then(guids).unwrap_or_default() {
            self.hierarchy_tree(c, out);
        }
    }

    /// The DOFs a controller's clip writes to, in order: (slot name, slot type), or `None`
    /// for the 8 unnamed DOFs of a delta-trajectory layout.
    fn dofs(&self, target: [u8; 16]) -> Result<Vec<Option<(String, i64)>>, Error> {
        let h = self.objects.get(&target).ok_or(Error::Bad("clip target missing"))?;
        let mut out = Vec::new();
        for l in h.get("LayoutAssets").and_then(guids).unwrap_or_default() {
            let layout = self.objects.get(&l).ok_or(Error::Bad("layout missing"))?;
            match layout.ty.as_str() {
                "DeltaTrajLayoutAsset" => out.extend(std::iter::repeat_n(None, 8)),
                _ => {
                    for s in layout.get("Slots").map(Gd::items).unwrap_or_default() {
                        let name = s.get("Name").and_then(Gd::str).unwrap_or_default().to_owned();
                        out.push(Some((name, s.get("Type").and_then(Gd::int).unwrap_or(0))));
                    }
                }
            }
        }
        Ok(out)
    }

    /// The clip a controller plays, decoded onto joints. Root motion (`AITrajectory`'s
    /// translation) is left out, since the game moves the unit itself; how far it carries
    /// the unit over the clip is kept as `Clip::travel`, so playback can match the stride
    /// to the unit's speed.
    pub fn clip(&self, controller: [u8; 16]) -> Result<Clip, Error> {
        let c = self.objects.get(&controller).ok_or(Error::Bad("no such clip controller"))?;
        let anim = self.get(c.guid("Anim")).ok_or(Error::Bad("controller without a clip"))?;
        let dofs = self.dofs(c.guid("Target").ok_or(Error::Bad("controller without a target"))?)?;
        let map = self.get(anim.guid("ChannelToDofAsset")).ok_or(Error::Bad("clip without a channel map"))?;
        let index: Vec<usize> = match (map.int("StorageType")?, map.get("IndexData")) {
            (1, Some(Gd::Bytes(b))) => b.iter().map(|&i| usize::from(i)).collect(),
            // Hierarchies of more than 255 DOFs: big-endian u16 per channel.
            (2, Some(Gd::Bytes(b))) => {
                b.as_chunks::<2>().0.iter().map(|&p| usize::from(u16::from_be_bytes(p))).collect()
            }
            _ => return Err(Error::Unsupported("channel map storage".into())),
        };
        let decoded = match anim.ty.as_str() {
            "VbrAnimationAsset" => vbr(anim)?,
            "RawAnimationAsset" => raw(anim)?,
            other => return Err(Error::Unsupported(format!("clip codec {other}"))),
        };
        let (nq, nv) = (decoded.quats.len(), decoded.vecs.len());
        if index.len() != nq + nv + decoded.floats {
            return Err(Error::Bad("channel map length differs from the clip's channels"));
        }
        let mut tracks = Vec::new();
        let mut travel = 0.0;
        for (c, &d) in index.iter().enumerate().take(nq + nv) {
            let Some(Some((slot, ty))) = dofs.get(d) else {
                if dofs.get(d).is_none() {
                    return Err(Error::Bad("channel past its DOFs"));
                }
                continue;
            };
            let (joint, suffix) = slot.rsplit_once('.').unwrap_or((slot, ""));
            let path = match (c < nq, *ty, suffix) {
                (true, ROTATION, "q") => Path::Rotation,
                (false, TRANSLATION, "t") => Path::Translation,
                (false, SCALE, "s") => Path::Scale,
                _ => return Err(Error::Bad("channel kind differs from its DOF")),
            };
            if joint == "AITrajectory" && path == Path::Translation {
                let keys = &decoded.vecs[c - nq];
                if let (Some(a), Some(b)) = (keys.first(), keys.last()) {
                    travel = ((b[0] - a[0]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
                }
                continue;
            }
            let keys = if c < nq {
                decoded.quats[c].clone()
            } else {
                decoded.vecs[c - nq].iter().map(|v| [v[0], v[1], v[2], 0.0]).collect()
            };
            tracks.push(Track { joint: joint.to_owned(), path, keys });
        }
        let fps = c.float("FPS")?;
        let frames = u16::try_from(anim.int("EndFrame")?).map_err(|_| Error::Bad("clip length"))?;
        if fps.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
            || decoded.times.last().is_some_and(|&t| t > frames)
        {
            return Err(Error::Bad("clip timing"));
        }
        Ok(Clip {
            name: c.name.clone(),
            fps,
            frames,
            times: decoded.times,
            tracks,
            travel,
            unit_error: decoded.unit_error,
        })
    }
}

fn guids(v: &Gd) -> Option<Vec<[u8; 16]>> {
    match v {
        Gd::List(l) => Some(l.iter().filter_map(Gd::guid).collect()),
        _ => None,
    }
}

/// A rig: its joints (name, parent's name) and the controllers of its clips.
pub struct Rig {
    pub name: String,
    pub joints: Vec<(String, Option<String>)>,
    pub clips: Vec<[u8; 16]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    Rotation,
    Translation,
    Scale,
}

/// One joint's channel in the joint's parent space (Frostbite's handedness): quaternions
/// `xyzw`, vectors `xyz_`. One key for a channel the clip holds constant, else one per
/// `Clip::times`.
#[derive(Clone, Debug)]
pub struct Track {
    pub joint: String,
    pub path: Path,
    pub keys: Vec<[f32; 4]>,
}

pub struct Clip {
    /// The controller's name, which is the clip's (`EU_RifleSoldier_AtkA`).
    pub name: String,
    pub fps: f32,
    /// The last frame: the clip lasts `frames / fps` seconds.
    pub frames: u16,
    /// The frame of each key.
    pub times: Vec<u16>,
    pub tracks: Vec<Track>,
    /// How far the clip's root motion carries the unit over its length (0 for a clip that
    /// stays in place).
    pub travel: f32,
    /// For each animated quaternion key, how far from unit length it decoded (before it was
    /// normalised): a check on the decode. Empty for raw clips.
    pub unit_error: Vec<f32>,
}

/// A clip's channels as decoded, before they meet the DOFs.
struct Decoded {
    times: Vec<u16>,
    quats: Vec<Vec<[f32; 4]>>,
    vecs: Vec<Vec<[f32; 3]>>,
    floats: usize,
    unit_error: Vec<f32>,
}

/// `RawAnimationAsset`: per key, every quaternion, then every vector (padded to four
/// floats), then the floats; `KeyTimes` gives each key's frame.
fn raw(a: &Object) -> Result<Decoded, Error> {
    let [nk, q, v, f] = ["NumKeys", "QuatCount", "Vec3Count", "FloatCount"]
        .map(|k| a.int(k).ok().and_then(|n| usize::try_from(n).ok()).unwrap_or(usize::MAX));
    let (Some(Gd::Ints(times)), Some(Gd::Floats(data))) = (a.get("KeyTimes"), a.get("Data")) else {
        return Err(Error::Bad("raw clip without keys"));
    };
    let stride = q.checked_mul(4).and_then(|x| x.checked_add(v.checked_mul(4)?)).and_then(|x| x.checked_add(f));
    if times.len() != nk || stride.and_then(|s| s.checked_mul(nk)) != Some(data.len()) {
        return Err(Error::Bad("raw clip sizes"));
    }
    let stride = stride.unwrap();
    let key = |k: usize, at: usize| -> [f32; 4] { std::array::from_fn(|i| data[k * stride + at + i]) };
    Ok(Decoded {
        times: times.iter().map(|&t| t as u16).collect(),
        quats: (0..q).map(|c| (0..nk).map(|k| normalized(key(k, 4 * c))).collect()).collect(),
        vecs: (0..v)
            .map(|c| {
                (0..nk)
                    .map(|k| {
                        let x = key(k, 4 * q + 4 * c);
                        [x[0], x[1], x[2]]
                    })
                    .collect()
            })
            .collect(),
        floats: f,
        unit_error: Vec::new(),
    })
}

fn normalized(q: [f32; 4]) -> [f32; 4] {
    let n = q.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 { q.map(|x| x / n) } else { [0.0, 0.0, 0.0, 1.0] }
}

/// The 8-point inverse DCT basis: `basis[r][k]` weighs coefficient `k` at key `r` of a block.
fn basis() -> [[f32; 8]; 8] {
    std::array::from_fn(|r| {
        std::array::from_fn(|k| {
            if k == 0 {
                0.125
            } else {
                ((std::f64::consts::PI * (2 * r + 1) as f64 * k as f64 / 16.0).cos() / 4.0) as f32
            }
        })
    })
}

/// `VbrAnimationAsset` ("VBR "). `Data` holds, in order: the key-time runs
/// (`KeyTimeSize` bytes), the constant channels' palette indices (4 per quaternion, 3 per
/// vector, 1 per float), the constant-channel map (`ConstChanMapSize` run lengths,
/// alternating animated and constant, starting animated), per animated scalar a u32 of
/// eight 4-bit coefficient widths, then the frame blocks (`FrameBlockSizes`).
///
/// A frame block covers 8 keys: three bytes choose the quantisation of rotations,
/// trajectory and other vectors, and the rest, stored byte-reversed and read LSB-first,
/// holds per scalar a presence bit for each coefficient of non-zero width, a sign bit for
/// each present one (1 = positive), then the magnitudes. Coefficient `k` is dequantised by
/// `(dct · 0.2 · (w + 1) · ln(k + 2) + 1) · dct / 32768`, the 8-point inverse DCT gives each
/// key a value in 0..1 (plus 0.5 from the DC term) and the channel kind's `Min`/`Max` scale it.
/// A last block of fewer than 8 keys holds the clip's last 8 keys, overlapping the block
/// before it: read that way, the step into it is the size of the clip's other steps (median
/// ratio 1.00 over 400 clips, against 1.72 if its keys are taken as the block's first).
fn vbr(a: &Object) -> Result<Decoded, Error> {
    let n =
        |k: &str| -> Result<usize, Error> { usize::try_from(a.int(k)?).map_err(|_| Error::Bad("negative VBR count")) };
    let (q, v, f) = (n("QuaternionCount")?, n("Vector3Count")?, n("FloatCount")?);
    let (cq, cv, cf) = (n("ConstQuaternionCount")?, n("ConstVector3Count")?, n("ConstFloatCount")?);
    let (nk, flags) = (n("NumKeys")?, n("Flags")?);
    if n("VectorOffsetSize")? != 0 || n("FloatOffsetSize")? != 0 {
        return Err(Error::Unsupported("VBR offset ramps".into()));
    }
    let (Some(Gd::Bytes(data)), Some(Gd::Ints(blocks)), Some(Gd::Floats(palette))) =
        (a.get("Data"), a.get("FrameBlockSizes"), a.get("ConstantPalette"))
    else {
        return Err(Error::Bad("VBR clip without data"));
    };
    let r = Le(data);
    let scalars = 4 * q + 3 * v + f;
    let groups = q + v + f.div_ceil(4);
    let kts = 0;
    let consts = kts + n("KeyTimeSize")?;
    let chan_map = consts + 4 * cq + 3 * cv + cf;
    let widths_at = chan_map + n("ConstChanMapSize")?;
    let mut block_at = widths_at + 4 * scalars;
    let total: i64 = blocks.iter().sum();
    if block_at as i64 + total != data.len() as i64 || blocks.len() != nk.div_ceil(8) && scalars > 0 {
        return Err(Error::Bad("VBR data size"));
    }
    if nk == 0 {
        return Err(Error::Bad("VBR clip without keys"));
    }

    // Key times: runs of keyed and skipped frames, or one key per frame without them.
    let runs = r.slice(kts, consts - kts)?;
    let times: Vec<u16> = if runs.is_empty() {
        (0..nk as u16).collect()
    } else {
        let mut t = Vec::with_capacity(nk);
        let mut frame = 0u16;
        for (i, &len) in runs.iter().enumerate() {
            if i % 2 == 0 {
                t.extend(frame..frame + u16::from(len));
            }
            frame += u16::from(len);
        }
        t
    };
    if times.len() != nk || flags & 2 != 0 && !runs.is_empty() {
        return Err(Error::Bad("VBR key times"));
    }

    // The animated scalars, key by key, as `lanes` floats: four per group (vectors padded).
    let lanes = 4 * groups;
    let mut keys = vec![0f32; nk.div_ceil(8) * 8 * lanes];
    let widths: Vec<u32> = (0..scalars).map(|s| r.u32(widths_at + 4 * s)).collect::<Result<_, _>>()?;
    let range = |kind: &str| -> Result<(f32, f32), Error> {
        let lo = a.float(&format!("{kind}Min"))?;
        Ok((lo, a.float(&format!("{kind}Max"))? - lo))
    };
    let ranges = [range("Quat")?, range("Traj")?, range("Vec3")?, range("Float")?];
    let dct = a.float("Dct")?;
    let basis = basis();
    let ln: [f32; 8] = std::array::from_fn(|k| ((k + 2) as f32).ln());
    for (b, &size) in blocks.iter().enumerate() {
        let size = usize::try_from(size).map_err(|_| Error::Bad("VBR block size"))?;
        let block = r.slice(block_at, size)?;
        block_at += size;
        if size < 3 {
            return Err(Error::Bad("VBR block shorter than its header"));
        }
        let body: Vec<u8> = block[3..].iter().rev().copied().collect();
        let mut bits = Bits { data: &body, at: 0 };
        let mut coeff = vec![[0i32; 8]; lanes];
        for (s, &w) in widths.iter().enumerate() {
            let width: [u32; 8] = std::array::from_fn(|c| (w >> (4 * c)) & 15);
            let present: [bool; 8] = std::array::from_fn(|c| width[c] != 0 && bits.bit() == 1);
            let positive: [bool; 8] = std::array::from_fn(|c| present[c] && bits.bit() == 1);
            // Vectors take four lanes each, floats follow them.
            let lane = if s < 4 * q {
                s
            } else if s < 4 * q + 3 * v {
                s + (s - 4 * q) / 3
            } else {
                s + v
            };
            for c in 0..8 {
                if present[c] {
                    let m = bits.read(width[c]) as i32;
                    coeff[lane][c] = if positive[c] { m } else { -m };
                }
            }
        }
        if bits.at > body.len() * 8 {
            return Err(Error::Bad("VBR block overrun"));
        }
        let step = |w: u8| -> [f32; 8] {
            let fw = (f32::from(w) + 1.0) * dct * 0.2;
            std::array::from_fn(|k| (fw * ln[k] + 1.0) * dct / 32768.0)
        };
        let rows = [step(block[0]), step(block[1]), step(block[2])];
        let first = if b > 0 && b == blocks.len() - 1 && nk % 8 != 0 { nk - 8 } else { b * 8 };
        for g in 0..groups {
            // Rotations, then the first vector (the trajectory), then the rest; the second
            // vector's first lane is quantised with the trajectory's row, as the game does.
            let (row, kind): ([usize; 4], usize) = if g < q {
                ([0; 4], 0)
            } else if g == q {
                ([1; 4], if v > 0 { 1 } else { 3 })
            } else if g == q + 1 {
                ([1, 2, 2, 2], if g < q + v { 2 } else { 3 })
            } else {
                ([2; 4], if g < q + v { 2 } else { 3 })
            };
            let (lo, span) = ranges[kind];
            for l in 0..4 {
                let c: [f32; 8] = std::array::from_fn(|k| coeff[4 * g + l][k] as f32 * rows[row[l]][k]);
                for (rr, basis) in basis.iter().enumerate() {
                    let key = first + rr;
                    if key < b * 8 {
                        continue;
                    }
                    let acc = 0.5 + (0..8).map(|k| basis[k] * c[k]).sum::<f32>();
                    keys[key * lanes + 4 * g + l] = acc * span + lo;
                }
            }
        }
    }

    // Constant channels: palette indices into `ConstantPalette`, scaled by their kind's range.
    let byte = |i: usize| r.slice(consts + i, 1).map(|b| b[0]);
    let value = |i: usize, (lo, span): (f32, f32)| -> Result<f32, Error> {
        let b = byte(i)?;
        let unit = if palette.is_empty() {
            f32::from(b) / 255.0
        } else {
            *palette.get(usize::from(b)).ok_or(Error::Bad("VBR palette index"))?
        };
        Ok(unit * span + lo)
    };
    let mut const_quats = Vec::with_capacity(cq);
    for j in 0..cq {
        let mut e = [0f32; 4];
        for (c, x) in e.iter_mut().enumerate() {
            *x = value(4 * j + c, ranges[0])?;
        }
        const_quats.push(normalized(e));
    }
    let mut const_vecs = Vec::with_capacity(cv);
    for j in 0..cv {
        let mut e = [0f32; 3];
        for (c, x) in e.iter_mut().enumerate() {
            *x = value(4 * cq + 3 * j + c, ranges[2])?;
        }
        const_vecs.push(e);
    }

    // Which channels are constant: runs alternating animated and constant.
    let total_channels = q + cq + v + cv + f + cf;
    let mut constant = Vec::with_capacity(total_channels);
    for (i, &len) in r.slice(chan_map, widths_at - chan_map)?.iter().enumerate() {
        constant.extend(std::iter::repeat_n(i % 2 == 1, usize::from(len)));
    }
    let tail = constant.last().copied().unwrap_or(false);
    constant.resize(total_channels, tail);
    let mut quats = Vec::with_capacity(q + cq);
    let mut unit_error = Vec::new();
    let (mut animated, mut fixed) = (0, 0);
    for &c in constant.iter().take(q + cq) {
        if c {
            quats.push(vec![*const_quats.get(fixed).ok_or(Error::Bad("VBR constant map"))?]);
            fixed += 1;
        } else {
            if animated >= q {
                return Err(Error::Bad("VBR constant map"));
            }
            let at = 4 * animated;
            let raw = (0..nk).map(|k| std::array::from_fn::<f32, 4, _>(|i| keys[k * lanes + at + i]));
            quats.push(
                raw.map(|q| {
                    unit_error.push((q.iter().map(|x| x * x).sum::<f32>().sqrt() - 1.0).abs());
                    normalized(q)
                })
                .collect(),
            );
            animated += 1;
        }
    }
    let mut vecs = Vec::with_capacity(v + cv);
    let (mut animated, mut fixed) = (0, 0);
    for j in 0..v + cv {
        if constant[q + cq + j] {
            vecs.push(vec![*const_vecs.get(fixed).ok_or(Error::Bad("VBR constant map"))?]);
            fixed += 1;
        } else {
            if animated >= v {
                return Err(Error::Bad("VBR constant map"));
            }
            let at = 4 * (q + animated);
            vecs.push((0..nk).map(|k| std::array::from_fn(|i| keys[k * lanes + at + i])).collect());
            animated += 1;
        }
    }
    Ok(Decoded { times, quats, vecs, floats: f + cf, unit_error })
}

/// LSB-first bits; reads past the end give zeros (and are caught by the caller).
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> u32 {
        let b = self.data.get(self.at >> 3).map_or(0, |b| u32::from(b >> (self.at & 7)) & 1);
        self.at += 1;
        b
    }
    fn read(&mut self, n: u32) -> u32 {
        (0..n).fold(0, |v, i| v | self.bit() << i)
    }
}

/// The layouts of `GD.REFL` at `at` (its body): a count, then an offset per layout. A layout
/// is min/max slot, size, alignment, string table offset and length, two flags and its hash,
/// then 32 bytes per slot (a zero size or count marks an unused one), a pad byte and its
/// name; field names are in its string table.
fn reflection(r: Le, at: usize) -> Result<BTreeMap<u32, Class>, Error> {
    let count = usize::try_from(r.i64(at)?).map_err(|_| Error::Bad("layout count"))?;
    let mut out = BTreeMap::new();
    for i in 0..count {
        let l = r.at(at, r.i64(at + 8 + 8 * i)?)?;
        let (min, max) = (r.i32(l)?, r.i32(l + 4)?);
        let (size, align, strings) = (r.u32(l + 8)? as usize, r.u32(l + 12)? as usize, r.u32(l + 16)? as usize);
        let hash = r.u32(l + 28)?;
        let slots = usize::try_from(i64::from(max) - i64::from(min) + 1).map_err(|_| Error::Bad("layout slots"))?;
        let mut fields = Vec::with_capacity(slots);
        for s in 0..slots {
            let p = l + 32 + 32 * s;
            let (elem, offset, name, used) = (r.u32(p + 4)?, r.u32(p + 8)?, r.u32(p + 12)?, r.u16(p + 16)?);
            if elem == 0 || used == 0 {
                continue;
            }
            let ty = r.u32(r.at(at, r.i64(p + 24)?)? + 28)?;
            let array = r.u16(p + 18)? & 1 == 1;
            fields.push(Field { name: r.cstr(l + strings + name as usize)?, offset: offset as usize, array, ty });
        }
        let name = r.cstr(l + 32 + 32 * slots + 1)?;
        out.insert(hash, Class { name, size, align: align.max(1), fields });
    }
    Ok(out)
}

/// One `GD.DATA` object at `at` (its body): a header naming its layout and where its fields
/// start, then the fields; `__base`, when set, points at a second object holding the base
/// class's part.
fn object(r: Le, at: usize, classes: &BTreeMap<u32, Class>) -> Result<Object, Error> {
    let header = |h: usize| -> Result<(u32, usize), Error> { Ok((r.u32(h + 16)?, usize::from(r.u16(h + 28)?))) };
    let (ty, offset) = header(at)?;
    let class = classes.get(&ty).ok_or(Error::Bad("object of an unknown layout"))?;
    let reader = Reader { r, base: at, classes };
    let Gd::Struct(mut fields) = reader.value(ty, at + offset, 0)? else { unreachable!() };
    if let Some(base) = fields.iter().find(|(k, _)| k == "__base").and_then(|(_, v)| v.int()).filter(|&b| b != 0) {
        let b = r.at(at, base)?;
        let (bty, boff) = header(b)?;
        if let Gd::Struct(more) = reader.value(bty, b + boff, 0)? {
            for (k, v) in more {
                if !fields.iter().any(|(f, _)| *f == k) {
                    fields.push((k, v));
                }
            }
        }
    }
    let name = fields
        .iter()
        .find(|(k, _)| k == "__name")
        .and_then(|(_, v)| v.str())
        .unwrap_or_default()
        .trim_end_matches('\0')
        .to_owned();
    Ok(Object { ty: class.name.clone(), name, fields: Gd::Struct(fields) })
}

struct Reader<'a> {
    r: Le<'a>,
    /// Where the object's section body starts: string and array pointers count from here.
    base: usize,
    classes: &'a BTreeMap<u32, Class>,
}

impl Reader<'_> {
    fn value(&self, ty: u32, at: usize, depth: u32) -> Result<Gd, Error> {
        if depth > 16 {
            return Err(Error::Bad("GenericData nested too deep"));
        }
        let class = self.classes.get(&ty).ok_or(Error::Bad("field of an unknown layout"))?;
        let mut out = Vec::with_capacity(class.fields.len());
        for f in &class.fields {
            let ft = self.classes.get(&f.ty).ok_or(Error::Bad("field of an unknown layout"))?;
            let p = at + f.offset;
            let v = if f.array {
                let count = self.r.u32(p + 4)? as usize;
                let data = self.r.at(self.base, self.r.i64(p + 8)?)?;
                self.array(f.ty, ft, data, count, depth)?
            } else {
                match self.primitive(&ft.name, p)? {
                    Some(v) => v,
                    None => self.value(f.ty, p, depth + 1)?,
                }
            };
            out.push((f.name.clone(), v));
        }
        Ok(Gd::Struct(out))
    }

    /// A primitive at `p`, or `None` for a class with fields of its own.
    fn primitive(&self, ty: &str, p: usize) -> Result<Option<Gd>, Error> {
        let r = self.r;
        Ok(Some(match ty {
            "Bool" => Gd::Bool(r.slice(p, 1)?[0] != 0),
            "Int8" => Gd::Int(i64::from(r.slice(p, 1)?[0] as i8)),
            "UInt8" => Gd::Int(i64::from(r.slice(p, 1)?[0])),
            "Int16" => Gd::Int(i64::from(r.u16(p)? as i16)),
            "UInt16" => Gd::Int(i64::from(r.u16(p)?)),
            "Int32" => Gd::Int(i64::from(r.i32(p)?)),
            "UInt32" => Gd::Int(i64::from(r.u32(p)?)),
            "Int64" | "UInt64" | "DataRef" => Gd::Int(r.i64(p)?),
            "Float" => Gd::Float(f64::from(f32::from_le_bytes(r.arr(p)?))),
            "Double" => Gd::Float(f64::from_le_bytes(r.arr(p)?)),
            "Guid" => Gd::Guid(r.arr(p)?),
            "String" => {
                let n = r.u32(p + 4)? as usize;
                let s = r.slice(r.at(self.base, r.i64(p + 8)?)?, n)?;
                Gd::Str(String::from_utf8_lossy(s).trim_end_matches('\0').to_owned())
            }
            _ => return Ok(None),
        }))
    }

    fn array(&self, ty: u32, class: &Class, at: usize, count: usize, depth: u32) -> Result<Gd, Error> {
        let r = self.r;
        Ok(match class.name.as_str() {
            "UInt8" | "Int8" | "Bool" => Gd::Bytes(r.slice(at, count)?.to_vec()),
            "Float" => {
                Gd::Floats(r.slice(at, 4 * count)?.as_chunks::<4>().0.iter().map(|&b| f32::from_le_bytes(b)).collect())
            }
            "Int16" => Gd::Ints(
                r.slice(at, 2 * count)?.as_chunks::<2>().0.iter().map(|&b| i64::from(i16::from_le_bytes(b))).collect(),
            ),
            "UInt16" => Gd::Ints(
                r.slice(at, 2 * count)?.as_chunks::<2>().0.iter().map(|&b| i64::from(u16::from_le_bytes(b))).collect(),
            ),
            "Int32" => Gd::Ints(
                r.slice(at, 4 * count)?.as_chunks::<4>().0.iter().map(|&b| i64::from(i32::from_le_bytes(b))).collect(),
            ),
            "UInt32" => Gd::Ints(
                r.slice(at, 4 * count)?.as_chunks::<4>().0.iter().map(|&b| i64::from(u32::from_le_bytes(b))).collect(),
            ),
            "Int64" | "UInt64" | "DataRef" => {
                Gd::Ints(r.slice(at, 8 * count)?.as_chunks::<8>().0.iter().map(|&b| i64::from_le_bytes(b)).collect())
            }
            "Guid" => Gd::List(r.slice(at, 16 * count)?.as_chunks::<16>().0.iter().map(|&b| Gd::Guid(b)).collect()),
            _ => {
                let stride = class.size.div_ceil(class.align) * class.align;
                if stride == 0 && count > 0 {
                    return Err(Error::Bad("array of empty elements"));
                }
                let mut items = Vec::with_capacity(count.min(1 << 16));
                for i in 0..count {
                    let p = at + i * stride;
                    items.push(match self.primitive(&class.name, p)? {
                        Some(v) => v,
                        None => self.value(ty, p, depth + 1)?,
                    });
                }
                Gd::List(items)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dct_basis_is_the_orthonormal_8_point_one_scaled_by_an_eighth() {
        let b = basis();
        assert_eq!(b[3][0], 0.125);
        // Column k of an orthogonal DCT basis: squared sums of 8·(c_k)² with c_0 = 1/8 and
        // c_k = 1/4 cos(...), i.e. 1/8 for k = 0 and 1/4 otherwise.
        for k in 0..8 {
            let s: f32 = b.iter().map(|row| row[k] * row[k]).sum();
            let want = if k == 0 { 8.0 / 64.0 } else { 0.25 };
            assert!((s - want).abs() < 1e-5, "column {k}: {s}");
        }
    }

    #[test]
    fn bits_read_lsb_first() {
        let data = [0b1011_0010u8, 0b0000_0001];
        let mut b = Bits { data: &data, at: 0 };
        assert_eq!(b.bit(), 0);
        assert_eq!(b.read(3), 0b001);
        assert_eq!(b.read(5), 0b1_1011);
        assert_eq!(b.bit(), 0);
    }
}
