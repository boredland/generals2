//! Which textures each mesh's materials use. The mesh assets' own materials carry no
//! texture parameters in this build; the bindings live in the per-level
//! `MeshVariationDatabase` assets, one entry per (mesh, variation) with a list of
//! `TextureShaderParameter`s per material. The parameter naming is not consistent
//! (`Diffuse`, `Di`, `_Diffuse`, ...), so the diffuse map is found by a list of names.
//!
//! Some meshes have no binding: no database entry, or one whose materials list no
//! parameters (the GLA oil derrick's static mesh in every level that has it). Their
//! blueprint also draws a sibling mesh (the skinned, build-up or damaged one) that is bound
//! and shares the material by name, so a slot left empty takes the map its siblings agree
//! on for that material name. This textures 40 meshes that had none.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::ebx::{Link, Value};
use crate::{Alpha, Error, mesh, res_type};

/// Parameter names that hold the colour map, most specific first.
const DIFFUSE: [&str; 5] = ["Diffuse", "Di", "_Diffuse", "diffuse", "Texture"];

/// For each mesh asset (by ebx name), the diffuse texture asset name per material slot.
/// Where levels disagree, the base variation (hash 0) with the most bindings wins. An ebx
/// asset that fails to parse goes to `failed` and is skipped (links to it don't resolve).
pub fn diffuse_maps(alpha: &Alpha, failed: &mut dyn FnMut(&str, Error)) -> BTreeMap<String, Vec<Option<String>>> {
    // Partition GUID -> ebx name, to resolve the databases' external links.
    let mut names = HashMap::new();
    let mut databases = Vec::new();
    // Per blueprint, the partitions it imports (its meshes among them).
    let mut blueprints = Vec::new();
    for name in alpha.ebx.keys() {
        let p = match alpha.asset(name) {
            Ok(p) => p,
            Err(e) => {
                failed(name, e);
                continue;
            }
        };
        names.insert(p.file_guid, name.clone());
        if name.ends_with("/meshvariationdb_win32") {
            databases.push(p);
        } else if p.primary().and_then(Value::ty).is_some_and(|t| t.ends_with("Blueprint")) {
            blueprints.push(p.imports.iter().map(|(file, _)| file).copied().collect::<Vec<_>>());
        }
    }
    let mut out: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
    for db in &databases {
        let external = |v: &Value| match v.link() {
            Some(Link::External(i)) => db.imports.get(i).and_then(|(file, _)| names.get(file)).cloned(),
            _ => None,
        };
        let Some(prim) = db.primary() else { continue };
        for entry in prim.get("Entries").map(Value::items).unwrap_or_default() {
            if entry.get("VariationAssetNameHash").and_then(Value::int) != Some(0) {
                continue;
            }
            let Some(mesh) = entry.get("Mesh").and_then(external) else { continue };
            let maps: Vec<Option<String>> = entry
                .get("Materials")
                .map(Value::items)
                .unwrap_or_default()
                .iter()
                .map(|m| {
                    let params = m.get("TextureParameters").map(Value::items).unwrap_or_default();
                    DIFFUSE.iter().find_map(|want| {
                        params
                            .iter()
                            .find(|p| p.get("ParameterName").and_then(Value::str) == Some(want))
                            .and_then(|p| p.get("Value").and_then(external))
                    })
                })
                .collect();
            let bound = maps.iter().flatten().count();
            let slot = out.entry(mesh).or_default();
            if bound > slot.iter().flatten().count() {
                *slot = maps;
            }
        }
    }
    let groups = blueprints.iter().map(|imports| {
        let meshes = imports.iter().filter_map(|f| names.get(f));
        meshes.filter(|n| alpha.res.get(*n).is_some_and(|e| e.res_type == Some(res_type::MESH_SET))).collect()
    });
    fill_from_siblings(alpha, &mut out, groups);
    out
}

/// Fills each empty slot of a mesh in a group from the group's other meshes: the map they
/// all bind to a material of the same name. Material names come from the MeshSet res; a
/// mesh that doesn't decode is left out here (the model export reports it).
fn fill_from_siblings<'a>(
    alpha: &Alpha,
    maps: &mut BTreeMap<String, Vec<Option<String>>>,
    groups: impl Iterator<Item = BTreeSet<&'a String>>,
) {
    let mut materials: BTreeMap<&String, Option<Vec<String>>> = BTreeMap::new();
    let mut slot_names = |mesh: &'a String| {
        materials
            .entry(mesh)
            .or_insert_with(|| {
                let m = mesh::read(alpha, &alpha.res[mesh]).ok()?;
                let mut slots = vec![String::new(); m.subsets.iter().map(|s| s.material_index + 1).max().unwrap_or(0)];
                for s in m.subsets.iter().filter(|s| !s.shadow_only) {
                    slots[s.material_index].clone_from(&s.material);
                }
                Some(slots)
            })
            .clone()
    };
    let mut fills = Vec::new();
    for group in groups {
        // Only groups with an empty slot pay for decoding their meshes. A mesh whose entries
        // bind nothing is kept as no slots at all.
        let unbound = |m: &&String| maps.get(*m).is_none_or(|b| b.is_empty() || b.iter().any(Option::is_none));
        let wanting: Vec<&String> = group.iter().copied().filter(unbound).collect();
        if wanting.is_empty() {
            continue;
        }
        // Material name -> every map a sibling binds to it.
        let mut offered: BTreeMap<String, BTreeSet<&String>> = BTreeMap::new();
        for &m in &group {
            let (Some(slots), Some(bound)) = (slot_names(m), maps.get(m)) else { continue };
            for (name, t) in slots.into_iter().zip(bound) {
                if let Some(t) = t.as_ref().filter(|_| !name.is_empty()) {
                    offered.entry(name).or_default().insert(t);
                }
            }
        }
        for m in wanting {
            let slots = slot_names(m).unwrap_or_default();
            let bound = maps.get(m).map(Vec::as_slice).unwrap_or_default();
            // Siblings that bind different maps to one name leave the slot empty: no guess.
            let filled: Vec<Option<String>> = slots
                .iter()
                .enumerate()
                .map(|(i, name)| match bound.get(i).cloned().flatten() {
                    Some(t) => Some(t),
                    None => {
                        offered.get(name).filter(|ts| ts.len() == 1).and_then(|ts| ts.first()).map(|t| t.to_string())
                    }
                })
                .collect();
            fills.push((m.clone(), filled));
        }
    }
    for (m, filled) in fills {
        let slot = maps.entry(m).or_default();
        if filled.iter().flatten().count() > slot.iter().flatten().count() {
            *slot = filled;
        }
    }
}
