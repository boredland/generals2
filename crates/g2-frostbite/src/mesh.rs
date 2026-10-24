//! MeshSet resources: a header (layout, one LOD in this build, subsets) followed inline by
//! vertex and index data, or with that data in a chunk the LOD names. Layout per
//! marv7000/IceBloc (`IceBlocLib.Frostbite2/Meshes`), with the LOD record's offsets checked
//! against the alpha: every one of its 2,154 meshes decodes inside its bounding box.
//!
//! Vertex elements are (usage, format, offset) triples per subset; positions and normals
//! are half-float, texture coordinates half2. Subsets are grouped into four categories by
//! index lists in the LOD; category 3 holds the shadow-depth proxy, which the importer
//! leaves out. Indices are 16-bit triangle lists.

use crate::{Alpha, Entry, Error, unblock};

pub struct MeshSet {
    pub name: String,
    pub bounds: ([f32; 3], [f32; 3]),
    pub subsets: Vec<Subset>,
}

pub struct Subset {
    pub material: String,
    /// Index into the mesh asset's materials (and so its texture bindings).
    pub material_index: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub uvs: Option<Vec<[f32; 2]>>,
    pub indices: Vec<u32>,
    /// Shadow-depth proxies (category 3); not drawn.
    pub shadow_only: bool,
}

const USAGE_POS: u8 = 1;
const USAGE_NORMAL: u8 = 6;
const USAGE_UV0: u8 = 0x21;
const FORMAT_HALF3: u8 = 7;
const FORMAT_HALF4: u8 = 8;
const FORMAT_HALF2: u8 = 6;
const SUBSET_SIZE: usize = 148;

/// Reads a MeshSet res: its `resMeta` starts with the header size, and the rest of the
/// payload is the inline geometry.
pub fn read(alpha: &Alpha, e: &Entry) -> Result<MeshSet, Error> {
    let meta = e.res_meta.ok_or(Error::Bad("MeshSet without resMeta"))?;
    let header_size = u32::from_le_bytes(meta[..4].try_into().unwrap()) as usize;
    let bytes = alpha.entry_bytes(e)?;
    let (hdr, body) = bytes.split_at_checked(header_size).ok_or(Error::Truncated)?;
    let r = Le(hdr);
    let lods = r.u32(8)? as usize;
    if lods == 0 {
        return Err(Error::Bad("mesh without LODs"));
    }
    let bounds = (r.vec3(16)?, r.vec3(32)?);
    let name = r.cstr(r.u32(88)? as usize)?;
    // LOD 0 only: the alpha ships one LOD per mesh.
    let lod = r.u32(48)? as usize;
    let subset_count = r.u32(lod + 8)? as usize;
    let subsets_at = r.u32(lod + 12)? as usize;
    let mut shadow = Vec::new();
    // Category lists: (count, pointer, pad) at +20; the fourth is the depth proxy.
    let (n, at) = (r.u32(lod + 20 + 36)? as usize, r.u32(lod + 24 + 36)? as usize);
    shadow.extend_from_slice(hdr.get(at..at + n).ok_or(Error::Truncated)?);
    let index_size = r.u32(lod + 76)? as usize;
    let vertex_size = r.u32(lod + 80)? as usize;
    let chunk: [u8; 16] = hdr.get(lod + 88..lod + 104).ok_or(Error::Truncated)?.try_into().unwrap();
    let chunk_data;
    let data: &[u8] = if chunk == [0; 16] {
        body
    } else {
        let raw = alpha.chunk_raw(&chunk)?;
        chunk_data = unblock(&raw).unwrap_or(raw);
        &chunk_data
    };
    if data.len() < vertex_size + index_size {
        return Err(Error::Bad("mesh geometry shorter than its LOD sizes"));
    }
    let (vertices, index_data) = data.split_at(vertex_size);
    let mut subsets = Vec::with_capacity(subset_count);
    for k in 0..subset_count {
        let at = subsets_at + SUBSET_SIZE * k;
        let material_at = r.u32(at + 8)? as usize;
        let material = if material_at == 0 { String::new() } else { r.cstr(material_at)? };
        let material_index = r.u32(at + 16)? as usize;
        let primitives = r.u32(at + 20)? as usize;
        let start = r.u32(at + 24)? as usize;
        let vertex_offset = r.u32(at + 28)? as usize;
        let count = r.u32(at + 32)? as usize;
        let stride = usize::from(r.u8(at + 36)?);
        let primitive_type = r.u8(at + 37)?;
        if primitive_type != 3 {
            return Err(Error::Unsupported(format!("{name}: primitive type {primitive_type}")));
        }
        let element_count = usize::from(r.u8(at + 120)?);
        let element = |usage: u8| -> Result<Option<(u8, usize)>, Error> {
            for i in 0..element_count.min(16) {
                let e = at + 48 + 4 * i;
                if r.u8(e)? == usage {
                    return Ok(Some((r.u8(e + 1)?, usize::from(r.u8(e + 2)?))));
                }
            }
            Ok(None)
        };
        let pos = element(USAGE_POS)?.ok_or(Error::Bad("subset without positions"))?;
        let normal = element(USAGE_NORMAL)?;
        let uv = element(USAGE_UV0)?;
        let verts = vertices
            .get(vertex_offset..vertex_offset + count * stride)
            .ok_or(Error::Bad("subset vertices past the vertex data"))?;
        let positions = halves::<3>(verts, stride, pos, &[FORMAT_HALF3, FORMAT_HALF4])?;
        let normals = normal.map(|n| halves::<3>(verts, stride, n, &[FORMAT_HALF4, FORMAT_HALF3])).transpose()?;
        let uvs = uv.map(|u| halves::<2>(verts, stride, u, &[FORMAT_HALF2])).transpose()?;
        let indices: Vec<u32> = index_data
            .get(start * 2..(start + primitives * 3) * 2)
            .ok_or(Error::Bad("subset indices past the index data"))?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u32::from(u16::from_le_bytes(*b)))
            .collect();
        if indices.iter().any(|&i| i as usize >= count) {
            return Err(Error::Bad("subset index past its vertices"));
        }
        subsets.push(Subset {
            material,
            material_index,
            positions,
            normals,
            uvs,
            indices,
            shadow_only: shadow.contains(&(k as u8)),
        });
    }
    Ok(MeshSet { name, bounds, subsets })
}

fn halves<const N: usize>(
    verts: &[u8],
    stride: usize,
    (format, offset): (u8, usize),
    allowed: &[u8],
) -> Result<Vec<[f32; N]>, Error> {
    if !allowed.contains(&format) {
        return Err(Error::Unsupported(format!("vertex element format {format}")));
    }
    if offset + 2 * N > stride {
        return Err(Error::Bad("vertex element past its stride"));
    }
    Ok(verts
        .chunks_exact(stride)
        .map(|v| std::array::from_fn(|i| f16_to_f32(u16::from_le_bytes([v[offset + 2 * i], v[offset + 2 * i + 1]]))))
        .collect())
}

/// IEEE half to single precision.
fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f32::from(h & 0x3ff);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 if frac == 0.0 => f32::INFINITY,
        31 => f32::NAN,
        _ => (1.0 + frac / 1024.0) * 2f32.powi(exp - 15),
    }
}

#[derive(Clone, Copy)]
struct Le<'a>(&'a [u8]);

impl Le<'_> {
    fn u8(&self, at: usize) -> Result<u8, Error> {
        self.0.get(at).copied().ok_or(Error::Truncated)
    }
    fn u32(&self, at: usize) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.0.get(at..at + 4).ok_or(Error::Truncated)?.try_into().unwrap()))
    }
    fn vec3(&self, at: usize) -> Result<[f32; 3], Error> {
        let mut v = [0.0; 3];
        for (i, c) in v.iter_mut().enumerate() {
            *c = f32::from_bits(self.u32(at + 4 * i)?);
        }
        Ok(v)
    }
    fn cstr(&self, at: usize) -> Result<String, Error> {
        let tail = self.0.get(at..).ok_or(Error::Truncated)?;
        let n = tail.iter().position(|&c| c == 0).ok_or(Error::Truncated)?;
        Ok(String::from_utf8_lossy(&tail[..n]).into_owned())
    }
}

/// A binary glTF (`.glb`) with one mesh, a primitive per drawn subset, and an embedded
/// PNG per material when `textures[material_index]` is given. Frostbite is left-handed
/// with +Z forward; glTF is right-handed, so Z is negated and the winding flipped.
pub fn to_glb(mesh: &MeshSet, textures: &[Option<Vec<u8>>]) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut prims = Vec::new();
    let mut view = |bin: &mut Vec<u8>, bytes: &[u8], target: Option<u32>| -> usize {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let at = bin.len();
        bin.extend_from_slice(bytes);
        views.push(match target {
            Some(t) => format!(r#"{{"buffer":0,"byteOffset":{at},"byteLength":{},"target":{t}}}"#, bytes.len()),
            None => format!(r#"{{"buffer":0,"byteOffset":{at},"byteLength":{}}}"#, bytes.len()),
        });
        views.len() - 1
    };
    let mut images = Vec::new();
    let mut materials = Vec::new();
    let mut material_of = std::collections::BTreeMap::new();
    for s in mesh.subsets.iter().filter(|s| !s.shadow_only && !s.indices.is_empty()) {
        let pos: Vec<[f32; 3]> = s.positions.iter().map(|p| [p[0], p[1], -p[2]]).collect();
        let (lo, hi) = pos.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
            (std::array::from_fn(|i| lo[i].min(p[i])), std::array::from_fn(|i| hi[i].max(p[i])))
        });
        let v = view(&mut bin, &f32_bytes(pos.iter().flatten()), Some(34962));
        accessors.push(format!(
            r#"{{"bufferView":{v},"componentType":5126,"count":{},"type":"VEC3","min":[{},{},{}],"max":[{},{},{}]}}"#,
            pos.len(),
            lo[0],
            lo[1],
            lo[2],
            hi[0],
            hi[1],
            hi[2]
        ));
        let mut attributes = format!(r#""POSITION":{}"#, accessors.len() - 1);
        if let Some(n) = &s.normals {
            let n: Vec<[f32; 3]> = n
                .iter()
                .map(|n| {
                    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
                    [n[0] / l, n[1] / l, -n[2] / l]
                })
                .collect();
            let v = view(&mut bin, &f32_bytes(n.iter().flatten()), Some(34962));
            accessors.push(format!(r#"{{"bufferView":{v},"componentType":5126,"count":{},"type":"VEC3"}}"#, n.len()));
            attributes += &format!(r#","NORMAL":{}"#, accessors.len() - 1);
        }
        if let Some(uv) = &s.uvs {
            let v = view(&mut bin, &f32_bytes(uv.iter().flatten()), Some(34962));
            accessors.push(format!(r#"{{"bufferView":{v},"componentType":5126,"count":{},"type":"VEC2"}}"#, uv.len()));
            attributes += &format!(r#","TEXCOORD_0":{}"#, accessors.len() - 1);
        }
        let flipped: Vec<u8> =
            s.indices.as_chunks::<3>().0.iter().flat_map(|t| [t[0], t[2], t[1]]).flat_map(u32::to_le_bytes).collect();
        let v = view(&mut bin, &flipped, Some(34963));
        accessors
            .push(format!(r#"{{"bufferView":{v},"componentType":5125,"count":{},"type":"SCALAR"}}"#, s.indices.len()));
        let indices = accessors.len() - 1;
        let material = *material_of.entry(s.material_index).or_insert_with(|| {
            let texture = textures.get(s.material_index).and_then(Option::as_ref).map(|png| {
                let v = view(&mut bin, png, None);
                images.push(format!(r#"{{"bufferView":{v},"mimeType":"image/png"}}"#));
                images.len() - 1
            });
            let pbr = match texture {
                Some(t) => format!(r#""baseColorTexture":{{"index":{t}}},"metallicFactor":0,"roughnessFactor":0.8"#),
                None => r#""baseColorFactor":[0.5,0.5,0.5,1],"metallicFactor":0,"roughnessFactor":0.8"#.to_owned(),
            };
            materials.push(format!(r#"{{"name":{},"pbrMetallicRoughness":{{{pbr}}}}}"#, json_str(&s.material)));
            materials.len() - 1
        });
        prims.push(format!(r#"{{"attributes":{{{attributes}}},"indices":{indices},"material":{material}}}"#));
    }
    let textures_json: Vec<String> = (0..images.len()).map(|i| format!(r#"{{"source":{i}}}"#)).collect();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let mut json = format!(
        r#"{{"asset":{{"version":"2.0","generator":"g2-frostbite"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0,"name":{name}}}],"meshes":[{{"name":{name},"primitives":[{}]}}],"materials":[{}],"accessors":[{}],"bufferViews":[{}],"buffers":[{{"byteLength":{}}}]"#,
        prims.join(","),
        materials.join(","),
        accessors.join(","),
        views.join(","),
        bin.len(),
        name = json_str(&mesh.name),
    );
    if !images.is_empty() {
        json += &format!(r#","images":[{}],"textures":[{}]"#, images.join(","), textures_json.join(","));
    }
    json.push('}');
    while !json.len().is_multiple_of(4) {
        json.push(' ');
    }
    let mut glb = Vec::with_capacity(28 + json.len() + bin.len());
    glb.extend(b"glTF");
    glb.extend(2u32.to_le_bytes());
    glb.extend(((28 + json.len() + bin.len()) as u32).to_le_bytes());
    glb.extend((json.len() as u32).to_le_bytes());
    glb.extend(b"JSON");
    glb.extend(json.as_bytes());
    glb.extend((bin.len() as u32).to_le_bytes());
    glb.extend(b"BIN\0");
    glb.extend(bin);
    glb
}

fn f32_bytes<'a>(v: impl Iterator<Item = &'a f32>) -> Vec<u8> {
    v.flat_map(|f| f.to_le_bytes()).collect()
}

fn json_str(s: &str) -> String {
    let mut out = String::from('"');
    for c in s.chars() {
        match c {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            c if (c as u32) < 0x20 => out += &format!("\\u{:04x}", c as u32),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_cover_normal_subnormal_and_special_values() {
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert_eq!(f16_to_f32(0x3555), 0.333_251_95);
        assert_eq!(f16_to_f32(0x0001), 2f32.powi(-24));
        assert!(f16_to_f32(0x7c00).is_infinite());
        assert!(f16_to_f32(0x7e00).is_nan());
    }

    fn triangle(shadow_only: bool) -> Subset {
        Subset {
            material: "m\"1".into(),
            material_index: 0,
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 2.0]],
            normals: Some(vec![[0.0, 0.0, -2.0]; 3]),
            uvs: Some(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]),
            indices: vec![0, 1, 2],
            shadow_only,
        }
    }

    #[test]
    fn glb_flips_handedness_and_skips_shadow_proxies() {
        let mesh = MeshSet {
            name: "tri".into(),
            bounds: ([0.0; 3], [1.0; 3]),
            subsets: vec![triangle(false), triangle(true)],
        };
        let glb = to_glb(&mesh, &[Some(vec![0x89, b'P', b'N', b'G'])]);
        assert_eq!(&glb[..4], b"glTF");
        assert_eq!(u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize, glb.len());
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json = std::str::from_utf8(&glb[20..20 + json_len]).unwrap();
        // One primitive (the shadow proxy is dropped), one embedded image, escaped names.
        assert_eq!(json.matches(r#""attributes""#).count(), 1);
        assert!(json.contains(r#""mimeType":"image/png""#));
        assert!(json.contains(r#""name":"m\"1""#));
        // Z negated: the third vertex's z bound is -2.
        assert!(json.contains(r#""min":[0,0,-2]"#), "{json}");
        let bin = &glb[20 + json_len + 8..];
        // Positions first (36 bytes), normals (36), uvs (24), then indices flipped 0,2,1.
        let idx: Vec<u32> = bin[96..108].chunks(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect();
        assert_eq!(idx, [0, 2, 1]);
        let nz = f32::from_le_bytes(bin[44..48].try_into().unwrap());
        assert_eq!(nz, 1.0, "normal normalised and negated");
    }
}
