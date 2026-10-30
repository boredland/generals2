//! MeshSet resources: a header (layout, one LOD in this build, subsets) followed inline by
//! vertex and index data, or with that data in a chunk the LOD names. Written from the
//! alpha's bytes, with marv7000/IceBloc's FB2 mesh reader (GPL-2.0) consulted for the field
//! names only; no code was taken. Every one of the alpha's 2,154 meshes decodes inside its
//! bounding box.
//!
//! Vertex elements are (usage, format, offset) triples per subset; positions and normals
//! are half-float, texture coordinates half2. Subsets are grouped into four categories by
//! index lists in the LOD; category 3 holds the shadow-depth proxy, which the importer
//! leaves out. Indices are 16-bit triangle lists.
//!
//! Skinned meshes add blend indices (4 x u8) and weights (4 x u8, summing to 255) per
//! vertex. An index is local to its subset: the subset header holds a bone palette (count
//! at +39, u16 skeleton bone indices at the header offset at +40) that maps it to a bone
//! of the unit's skeleton. Checked over all 172 skinned meshes: every weight sums to 255
//! and no weighted index falls outside its palette.

use crate::skeleton::Skeleton;
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
    /// Skinned meshes: per vertex, four skeleton bone indices and their weights (sum 1).
    pub skin: Option<Vec<([u16; 4], [f32; 4])>>,
    pub indices: Vec<u32>,
    /// Shadow-depth proxies (category 3); not drawn.
    pub shadow_only: bool,
}

const USAGE_POS: u8 = 1;
const USAGE_NORMAL: u8 = 6;
const USAGE_UV0: u8 = 0x21;
const USAGE_BLEND_INDICES: u8 = 2;
const USAGE_BLEND_WEIGHTS: u8 = 4;
const FORMAT_UBYTE4: u8 = 12;
const FORMAT_UBYTE4N: u8 = 13;
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
        let skin = match (element(USAGE_BLEND_INDICES)?, element(USAGE_BLEND_WEIGHTS)?) {
            (Some((FORMAT_UBYTE4, io)), Some((FORMAT_UBYTE4N, wo))) if io + 4 <= stride && wo + 4 <= stride => {
                let count = usize::from(r.u8(at + 39)?);
                let palette_at = r.u32(at + 40)? as usize;
                let palette: Vec<u16> = hdr
                    .get(palette_at..palette_at + 2 * count)
                    .ok_or(Error::Truncated)?
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes(*b))
                    .collect();
                let skin = verts
                    .chunks_exact(stride)
                    .map(|v| {
                        let mut bones = [0u16; 4];
                        let mut weights = [0f32; 4];
                        for j in 0..4 {
                            let w = v[wo + j];
                            if w > 0 {
                                bones[j] = *palette
                                    .get(usize::from(v[io + j]))
                                    .ok_or(Error::Bad("blend index past its palette"))?;
                                weights[j] = f32::from(w) / 255.0;
                            }
                        }
                        Ok((bones, weights))
                    })
                    .collect::<Result<Vec<_>, Error>>()?;
                Some(skin)
            }
            // Rigid meshes carry one bone element or other formats: drawn unskinned.
            _ => None,
        };
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
            skin,
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

/// A binary glTF (`.glb`) with one mesh and a primitive per drawn subset. A material whose
/// `textures[material_index]` is given uses that image, by URI relative to the `.glb` (the
/// import writes each texture once and every model that uses it points there). Frostbite
/// is left-handed and models face +Z (a tank's barrel points there), as the client's do;
/// glTF is right-handed, so X is negated (keeping +Z forward) and the winding flipped.
///
/// With a `skeleton` (and skin data on the subsets), the mesh is skinned: one joint node
/// per bone, named as the bone, in its bind pose, so the model looks as unskinned until a
/// joint is moved. Bone transforms are mirrored the same way (X negated on both sides).
pub fn to_glb(mesh: &MeshSet, textures: &[Option<String>], skeleton: Option<&Skeleton>) -> Vec<u8> {
    let skeleton = skeleton.filter(|_| mesh.subsets.iter().any(|s| !s.shadow_only && s.skin.is_some()));
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
        let pos: Vec<[f32; 3]> = s.positions.iter().map(|p| [-p[0], p[1], p[2]]).collect();
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
                    [-n[0] / l, n[1] / l, n[2] / l]
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
        // Every primitive of a skinned mesh needs joints; a subset without skin data rides
        // the root bone.
        if let Some(sk) = skeleton {
            let n = s.positions.len();
            let skin: Vec<([u16; 4], [f32; 4])> =
                s.skin.clone().unwrap_or_else(|| vec![([0; 4], [1.0, 0.0, 0.0, 0.0]); n]);
            let bones = sk.names.len() as u16;
            let joints: Vec<u8> = skin
                .iter()
                .flat_map(|(b, w)| {
                    std::array::from_fn::<u16, 4, _>(|j| if w[j] > 0.0 && b[j] < bones { b[j] } else { 0 })
                })
                .flat_map(u16::to_le_bytes)
                .collect();
            let v = view(&mut bin, &joints, Some(34962));
            accessors.push(format!(r#"{{"bufferView":{v},"componentType":5123,"count":{n},"type":"VEC4"}}"#));
            attributes += &format!(r#","JOINTS_0":{}"#, accessors.len() - 1);
            let weights: Vec<f32> = skin
                .iter()
                .flat_map(|(_, w)| {
                    let sum: f32 = w.iter().sum();
                    w.map(|x| if sum > 0.0 { x / sum } else { 0.0 })
                })
                .collect();
            let v = view(&mut bin, &f32_bytes(weights.iter()), Some(34962));
            accessors.push(format!(r#"{{"bufferView":{v},"componentType":5126,"count":{n},"type":"VEC4"}}"#));
            attributes += &format!(r#","WEIGHTS_0":{}"#, accessors.len() - 1);
        }
        let flipped: Vec<u8> =
            s.indices.as_chunks::<3>().0.iter().flat_map(|t| [t[0], t[2], t[1]]).flat_map(u32::to_le_bytes).collect();
        let v = view(&mut bin, &flipped, Some(34963));
        accessors
            .push(format!(r#"{{"bufferView":{v},"componentType":5125,"count":{},"type":"SCALAR"}}"#, s.indices.len()));
        let indices = accessors.len() - 1;
        let material = *material_of.entry(s.material_index).or_insert_with(|| {
            let texture = textures.get(s.material_index).and_then(Option::as_ref).map(|uri| {
                images.push(format!(r#"{{"uri":{},"mimeType":"image/png"}}"#, json_str(uri)));
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
    // Node 0 holds the mesh (and skin); joints are nodes 1.., bone i at node i + 1, each
    // child of its parent bone; the root bones are scene roots beside node 0.
    let mut nodes = vec![match skeleton {
        Some(_) => format!(r#"{{"mesh":0,"skin":0,"name":{}}}"#, json_str(&mesh.name)),
        None => format!(r#"{{"mesh":0,"name":{}}}"#, json_str(&mesh.name)),
    }];
    let mut scene_roots = vec![0];
    let mut skin_json = String::new();
    if let Some(sk) = skeleton {
        let model: Vec<[f32; 16]> = sk.model_pose.iter().map(mirror_x).collect();
        for (i, name) in sk.names.iter().enumerate() {
            let local = match sk.parents[i] {
                Some(p) => mul(&invert_rigid(&model[p]), &model[i]),
                None => model[i],
            };
            let children: Vec<String> =
                (0..sk.names.len()).filter(|&c| sk.parents[c] == Some(i)).map(|c| (c + 1).to_string()).collect();
            let children =
                if children.is_empty() { String::new() } else { format!(r#","children":[{}]"#, children.join(",")) };
            nodes.push(format!(r#"{{"name":{},"matrix":[{}]{children}}}"#, json_str(name), floats(&local)));
            if sk.parents[i].is_none() {
                scene_roots.push(i + 1);
            }
        }
        let inverse: Vec<f32> = model.iter().flat_map(invert_rigid).collect();
        let v = view(&mut bin, &f32_bytes(inverse.iter()), None);
        accessors.push(format!(r#"{{"bufferView":{v},"componentType":5126,"count":{},"type":"MAT4"}}"#, model.len()));
        let joints: Vec<String> = (1..=sk.names.len()).map(|n| n.to_string()).collect();
        skin_json =
            format!(r#","skins":[{{"inverseBindMatrices":{},"joints":[{}]}}]"#, accessors.len() - 1, joints.join(","));
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let scene_roots: Vec<String> = scene_roots.iter().map(usize::to_string).collect();
    let mut json = format!(
        r#"{{"asset":{{"version":"2.0","generator":"g2-frostbite"}},"scene":0,"scenes":[{{"nodes":[{}]}}],"nodes":[{}],"meshes":[{{"name":{name},"primitives":[{}]}}],"materials":[{}],"accessors":[{}],"bufferViews":[{}],"buffers":[{{"byteLength":{}}}]{skin_json}"#,
        scene_roots.join(","),
        nodes.join(","),
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
    glb(json, bin)
}

/// The binary container: the JSON chunk (space-padded) and the binary chunk (4-aligned).
fn glb(mut json: String, mut bin: Vec<u8>) -> Vec<u8> {
    while !json.len().is_multiple_of(4) {
        json.push(' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
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

/// A binary glTF of `skeleton`'s joints alone, nodes named, nested and placed as `to_glb`
/// writes them (so the paths of names from the root joint, which is what an animation
/// targets, are the same as the skinned model's), with each clip as a named animation.
///
/// The ANT rigs turn `AITrajectory` a quarter turn that the skeletons' bind poses don't
/// have, and their joints below it turn it back: here its rotation is folded into the
/// joints below it and it keeps its bind pose. Over 475 joints of every rig, folded clips
/// sit a median 16 degrees from the bind pose at their first key, unfolded 90. Root motion
/// (`AITrajectory`'s translation) is not in the clips. Joints a clip leaves out hold their
/// bind pose, so switching clips never leaves a joint where the last one put it. Values
/// are mirrored in X like the bones (a quaternion's y and z, a translation's x negate).
pub fn clips_to_glb(skeleton: &Skeleton, clips: &[crate::ant::Clip]) -> Vec<u8> {
    use crate::ant::Path;
    let sk = skeleton;
    let model: Vec<[f32; 16]> = sk.model_pose.iter().map(mirror_x).collect();
    let local: Vec<[f32; 16]> = (0..sk.names.len())
        .map(|i| match sk.parents[i] {
            Some(p) => mul(&invert_rigid(&model[p]), &model[i]),
            None => model[i],
        })
        .collect();
    // Joints as rotation and translation (glTF forbids animating a node given by `matrix`).
    let mut nodes = Vec::new();
    for (i, name) in sk.names.iter().enumerate() {
        let children: Vec<String> =
            (0..sk.names.len()).filter(|&c| sk.parents[c] == Some(i)).map(|c| c.to_string()).collect();
        let children =
            if children.is_empty() { String::new() } else { format!(r#","children":[{}]"#, children.join(",")) };
        nodes.push(format!(
            r#"{{"name":{},"rotation":[{}],"translation":[{}]{children}}}"#,
            json_str(name),
            floats(&quat_of(&local[i])),
            floats(&local[i][12..15])
        ));
    }
    let roots: Vec<String> = (0..sk.names.len()).filter(|&i| sk.parents[i].is_none()).map(|i| i.to_string()).collect();
    let traj = sk.names.iter().position(|n| n == "AITrajectory");
    let mut bin: Vec<u8> = Vec::new();
    let (mut views, mut accessors, mut animations) = (Vec::new(), Vec::new(), Vec::new());
    let mut push = |bin: &mut Vec<u8>, data: &[f32], ty: &str, count: usize, minmax: bool| -> usize {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        views.push(format!(r#"{{"buffer":0,"byteOffset":{},"byteLength":{}}}"#, bin.len(), 4 * data.len()));
        bin.extend(f32_bytes(data.iter()));
        let bounds = if minmax {
            let (lo, hi) = data.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &t| (lo.min(t), hi.max(t)));
            format!(r#","min":[{lo}],"max":[{hi}]"#)
        } else {
            String::new()
        };
        accessors.push(format!(
            r#"{{"bufferView":{},"componentType":5126,"count":{count},"type":"{ty}"{bounds}}}"#,
            views.len() - 1
        ));
        accessors.len() - 1
    };
    // Every constant track shares one input: a single key at 0.
    let at_zero = push(&mut bin, &[0.0], "SCALAR", 1, true);
    for clip in clips {
        let n = clip.times.len();
        let times: Vec<f32> = clip.times.iter().map(|&t| f32::from(t) / clip.fps).collect();
        let input = push(&mut bin, &times, "SCALAR", n, true);
        let track = |bone: usize, path: Path| {
            clip.tracks.iter().find(|t| t.path == path && t.joint == sk.names[bone]).map(|t| t.keys.as_slice())
        };
        // The trajectory's turn as the clip has it, relative to its bind pose (which it keeps):
        // `bind⁻¹ · key`, in Frostbite's space, carried into the joints below it.
        let traj_bind = traj.map(|t| {
            let local = match sk.parents[t] {
                Some(p) => mul(&invert_rigid(&sk.model_pose[p]), &sk.model_pose[t]),
                None => sk.model_pose[t],
            };
            let q = quat_of(&local);
            [-q[0], -q[1], -q[2], q[3]]
        });
        let traj_rot: Option<Vec<[f32; 4]>> = traj
            .and_then(|t| track(t, Path::Rotation))
            .map(|keys| keys.iter().map(|&k| quat_mul(traj_bind.unwrap(), k)).collect());
        let traj_rot = traj_rot.as_deref();
        let (mut channels, mut samplers) = (Vec::new(), Vec::new());
        for (bone, rest) in local.iter().enumerate() {
            let folded = traj.is_some() && sk.parents[bone] == traj;
            let fold = |k: usize| traj_rot.filter(|_| folded).map(|r| r[k.min(r.len() - 1)]);
            let (rot, trans) = if Some(bone) == traj {
                (None, None)
            } else {
                (track(bone, Path::Rotation), track(bone, Path::Translation))
            };
            // A folded joint takes as many keys as it or the trajectory has.
            let keys_of = |keys: Option<&[[f32; 4]]>| {
                let keys = keys?;
                Some(keys.len().max(if folded { traj_rot.map_or(0, <[_]>::len) } else { 0 }))
            };
            let mut out_rot = Vec::new();
            match (rot, keys_of(rot)) {
                (Some(keys), Some(len)) => {
                    for k in 0..len {
                        let q = keys[k.min(keys.len() - 1)];
                        let q = fold(k).map_or(q, |r| quat_mul(r, q));
                        out_rot.extend([q[0], -q[1], -q[2], q[3]]);
                    }
                }
                _ => out_rot.extend(quat_of(rest)),
            }
            let mut out_trans = Vec::new();
            match (trans, keys_of(trans)) {
                (Some(keys), Some(len)) => {
                    for k in 0..len {
                        let t = keys[k.min(keys.len() - 1)];
                        let t = fold(k).map_or([t[0], t[1], t[2]], |r| quat_rotate(r, [t[0], t[1], t[2]]));
                        out_trans.extend([-t[0], t[1], t[2]]);
                    }
                }
                _ => out_trans.extend(&rest[12..15]),
            }
            for (data, path, ty, width) in [(&out_rot, "rotation", "VEC4", 4), (&out_trans, "translation", "VEC3", 3)] {
                let count = data.len() / width;
                let output = push(&mut bin, data, ty, count, false);
                let input = if count == 1 { at_zero } else { input };
                samplers.push(format!(r#"{{"input":{input},"output":{output},"interpolation":"LINEAR"}}"#));
                channels.push(format!(
                    r#"{{"sampler":{},"target":{{"node":{bone},"path":"{path}"}}}}"#,
                    samplers.len() - 1
                ));
            }
        }
        animations.push(format!(
            r#"{{"name":{},"channels":[{}],"samplers":[{}]}}"#,
            json_str(&clip.name),
            channels.join(","),
            samplers.join(",")
        ));
    }
    let json = format!(
        r#"{{"asset":{{"version":"2.0","generator":"g2-frostbite"}},"scene":0,"scenes":[{{"nodes":[{}]}}],"nodes":[{}],"animations":[{}],"accessors":[{}],"bufferViews":[{}],"buffers":[{{"byteLength":{}}}]}}"#,
        roots.join(","),
        nodes.join(","),
        animations.join(","),
        accessors.join(","),
        views.join(","),
        bin.len().next_multiple_of(4),
    );
    glb(json, bin)
}

/// Hamilton product `a·b` of `xyzw` quaternions.
fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// `v` turned by the unit quaternion `q`.
fn quat_rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let r = quat_mul(quat_mul(q, [v[0], v[1], v[2], 0.0]), [-q[0], -q[1], -q[2], q[3]]);
    [r[0], r[1], r[2]]
}

/// The rotation of a rigid column-major 4x4 as an `xyzw` quaternion.
fn quat_of(m: &[f32; 16]) -> [f32; 4] {
    let e = |row: usize, col: usize| m[col * 4 + row];
    let trace = e(0, 0) + e(1, 1) + e(2, 2);
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [(e(2, 1) - e(1, 2)) / s, (e(0, 2) - e(2, 0)) / s, (e(1, 0) - e(0, 1)) / s, s / 4.0]
    } else {
        let i = if e(0, 0) >= e(1, 1) && e(0, 0) >= e(2, 2) {
            0
        } else if e(1, 1) >= e(2, 2) {
            1
        } else {
            2
        };
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        let s = (1.0 + e(i, i) - e(j, j) - e(k, k)).sqrt() * 2.0;
        let mut q = [0.0; 4];
        q[i] = s / 4.0;
        q[j] = (e(j, i) + e(i, j)) / s;
        q[k] = (e(k, i) + e(i, k)) / s;
        q[3] = (e(k, j) - e(j, k)) / s;
        q
    };
    let n = q.iter().map(|x| x * x).sum::<f32>().sqrt();
    q.map(|x| x / n)
}

/// `S·m·S` with S = diag(-1, 1, 1, 1): a transform in Frostbite's handedness as glTF's.
fn mirror_x(m: &[f32; 16]) -> [f32; 16] {
    let mut out = *m;
    for col in 0..4 {
        for row in 0..4 {
            if (row == 0) != (col == 0) {
                out[col * 4 + row] = -m[col * 4 + row];
            }
        }
    }
    out
}

/// Column-major 4x4 product `a·b`.
fn mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    std::array::from_fn(|i| {
        let (col, row) = (i / 4, i % 4);
        (0..4).map(|k| a[k * 4 + row] * b[col * 4 + k]).sum()
    })
}

/// Inverse of a rotation-plus-translation (the bind poses are orthonormal): transpose the
/// rotation, rotate the translation back.
fn invert_rigid(m: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0; 16];
    for col in 0..3 {
        for row in 0..3 {
            out[col * 4 + row] = m[row * 4 + col];
        }
    }
    for row in 0..3 {
        out[12 + row] = -(0..3).map(|k| out[k * 4 + row] * m[12 + k]).sum::<f32>();
    }
    out[15] = 1.0;
    out
}

fn floats(m: &[f32]) -> String {
    m.iter().map(f32::to_string).collect::<Vec<_>>().join(",")
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
            skin: None,
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
        let glb = to_glb(&mesh, &[Some("../../textures/a b%.png".into())], None);
        assert_eq!(&glb[..4], b"glTF");
        assert_eq!(u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize, glb.len());
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json = std::str::from_utf8(&glb[20..20 + json_len]).unwrap();
        // One primitive (the shadow proxy is dropped), one image by URI (not embedded, so
        // the binary chunk holds geometry only), escaped names.
        assert_eq!(json.matches(r#""attributes""#).count(), 1);
        assert!(json.contains(r#""images":[{"uri":"../../textures/a b%.png","mimeType":"image/png"}]"#));
        assert_eq!(json.matches(r#""bufferView":"#).count(), 4, "positions, normals, uvs, indices");
        assert!(json.contains(r#""name":"m\"1""#));
        // X negated, Z kept: positions span x -1..0 and z 0..2.
        let bound = |key: &str| -> Vec<f32> {
            let at = json.find(key).unwrap() + key.len();
            json[at..json[at..].find(']').unwrap() + at].split(',').map(|v| v.parse().unwrap()).collect()
        };
        assert_eq!((bound(r#""min":["#), bound(r#""max":["#)), (vec![-1.0, 0.0, 0.0], vec![0.0, 1.0, 2.0]));
        let bin = &glb[20 + json_len + 8..];
        // Positions first (36 bytes), normals (36), uvs (24), then indices flipped 0,2,1.
        let idx: Vec<u32> = bin[96..108].chunks(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect();
        assert_eq!(idx, [0, 2, 1]);
        let nx = f32::from_le_bytes(bin[36..40].try_into().unwrap());
        let nz = f32::from_le_bytes(bin[44..48].try_into().unwrap());
        assert_eq!((nx, nz), (0.0, -1.0), "normal normalised, z kept");
    }

    /// Skinned export: each joint node's local matrix times its parents' gives the bone's
    /// mirrored model pose, and inverse bind times that pose is identity, so at rest every
    /// vertex stays where the static export puts it. Bone 1 here is rotated and offset.
    #[test]
    fn skinned_bind_pose_leaves_vertices_in_place() {
        let rot = |a: f32| -> [f32; 16] {
            let (s, c) = a.sin_cos();
            [c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0]
        };
        let mut b1 = rot(0.7);
        b1[12..15].copy_from_slice(&[0.5, 2.0, -1.0]);
        let mut b2 = mul(&b1, &rot(-0.3));
        b2[12..15].copy_from_slice(&[1.0, 2.5, 0.0]);
        let sk = Skeleton {
            names: vec!["Reference".into(), "Turret".into(), "Pitch".into()],
            parents: vec![None, Some(0), Some(1)],
            model_pose: vec![rot(0.0), b1, b2],
        };
        let mut tri = triangle(false);
        tri.skin = Some(vec![([2, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]); 3]);
        let mesh = MeshSet { name: "t".into(), bounds: ([0.0; 3], [1.0; 3]), subsets: vec![tri] };
        let glb = to_glb(&mesh, &[], Some(&sk));
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json = std::str::from_utf8(&glb[20..20 + json_len]).unwrap();
        assert!(json.contains(r#""skins":[{"inverseBindMatrices":"#) && json.contains(r#""JOINTS_0""#));
        assert!(json.contains(r#""name":"Turret""#) && json.contains(r#""children":[3]"#), "Pitch under Turret");
        // Rebuild Pitch's world matrix from the node chain and check it is the mirrored pose.
        let model: Vec<[f32; 16]> = sk.model_pose.iter().map(mirror_x).collect();
        let local = |i: usize| match sk.parents[i] {
            Some(p) => mul(&invert_rigid(&model[p]), &model[i]),
            None => model[i],
        };
        let world = mul(&mul(&local(0), &local(1)), &local(2));
        let skinning = mul(&world, &invert_rigid(&model[2]));
        let identity: [f32; 16] = std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.0 });
        assert!(skinning.iter().zip(identity).all(|(a, b)| (a - b).abs() < 1e-5), "{skinning:?}");
        assert!(world.iter().zip(model[2]).all(|(a, b)| (a - b).abs() < 1e-5));
    }

    /// The clip file: joints by name as TRS nodes at their bind pose, and per clip a named
    /// animation over every joint. A clip key equal to the bind pose writes the node's own
    /// rotation and translation (mirrored as the bones are), a joint the clip leaves out
    /// holds its bind pose, and a trajectory turn of a quarter turn is folded into the joint
    /// below it: the trajectory's channel stays at its bind pose, the child takes the turn.
    #[test]
    fn clip_file_keys_mirror_like_the_bones_and_fold_the_trajectory() {
        use crate::ant::{Clip, Path, Track};
        let rot = |a: f32| -> [f32; 16] {
            let (s, c) = a.sin_cos();
            [c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0]
        };
        let mut hips = rot(0.4);
        hips[12..15].copy_from_slice(&[0.3, 1.0, -0.2]);
        let sk = Skeleton {
            names: vec!["Reference".into(), "AITrajectory".into(), "Hips".into(), "Head".into()],
            parents: vec![None, Some(0), Some(1), Some(2)],
            model_pose: vec![rot(0.0), rot(0.0), hips, hips],
        };
        // Hips' bind pose in Frostbite's space, as a clip would key it.
        let q = quat_of(&hips);
        let quarter = [0.0, -std::f32::consts::FRAC_1_SQRT_2, 0.0, std::f32::consts::FRAC_1_SQRT_2];
        let clip = Clip {
            name: "Run".into(),
            fps: 30.0,
            frames: 15,
            times: vec![0, 15],
            tracks: vec![
                Track { joint: "Hips".into(), path: Path::Rotation, keys: vec![q, q] },
                Track { joint: "Hips".into(), path: Path::Translation, keys: vec![[0.3, 1.0, -0.2, 0.0]; 2] },
                Track { joint: "AITrajectory".into(), path: Path::Rotation, keys: vec![quarter; 2] },
            ],
            travel: 0.0,
            unit_error: Vec::new(),
        };
        let glb = clips_to_glb(&sk, &[clip]);
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json = std::str::from_utf8(&glb[20..20 + json_len]).unwrap();
        let bin = &glb[20 + json_len + 8..];
        assert!(json.contains(r#""animations":[{"name":"Run""#) && !json.contains(r#""matrix""#));
        // Four joints, two channels each.
        assert_eq!(json.matches(r#""target":{"node":"#).count(), 8);
        // Accessor `i`'s floats, through its buffer view.
        let accessors: Vec<&str> = json.split(r#"{"bufferView":"#).skip(1).collect();
        let views: Vec<&str> = json.split(r#"{"buffer":0,"byteOffset":"#).skip(1).collect();
        let floats = |sampler_output: usize| -> Vec<f32> {
            let a = accessors[sampler_output];
            let view: usize = a[..a.find(',').unwrap()].parse().unwrap();
            let v = views[view];
            let offset: usize = v[..v.find(',').unwrap()].parse().unwrap();
            let len: usize = v[v.find("byteLength\":").unwrap() + 12..v.find('}').unwrap()].parse().unwrap();
            bin[offset..offset + len].chunks(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect()
        };
        // The samplers of joint `bone`: rotation then translation, in joint order.
        let outputs: Vec<usize> =
            json.split(r#""output":"#).skip(1).map(|s| s[..s.find(',').unwrap()].parse().unwrap()).collect();
        let close = |a: &[f32], b: &[f32]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
        let model: Vec<[f32; 16]> = sk.model_pose.iter().map(mirror_x).collect();
        let local = |i: usize| match sk.parents[i] {
            Some(p) => mul(&invert_rigid(&model[p]), &model[i]),
            None => model[i],
        };
        // The trajectory holds its bind pose (identity here) despite its clip key.
        assert!(close(&floats(outputs[2]), &quat_of(&local(1))), "trajectory kept at bind");
        // Hips: the bind-pose key, turned by the folded quarter turn: a quarter turn about
        // Y on top of its bind rotation, mirrored (Y and Z negate, so the turn flips sign).
        let hips_rot = floats(outputs[4]);
        let turned =
            quat_mul([0.0, std::f32::consts::FRAC_1_SQRT_2, 0.0, std::f32::consts::FRAC_1_SQRT_2], quat_of(&local(2)));
        assert!(close(&hips_rot[..4], &turned), "{hips_rot:?} against {turned:?}");
        // Head is in no track: it holds its bind pose, one key.
        assert!(close(&floats(outputs[6]), &quat_of(&local(3))) && close(&floats(outputs[7]), &local(3)[12..15]));
    }
}
