//! Which textures each mesh's materials use. The mesh assets' own materials carry no
//! texture parameters in this build; the bindings live in the per-level
//! `MeshVariationDatabase` assets, one entry per (mesh, variation) with a list of
//! `TextureShaderParameter`s per material. The parameter naming is not consistent
//! (`Diffuse`, `Di`, `_Diffuse`, ...), so the diffuse map is found by a list of names.

use std::collections::{BTreeMap, HashMap};

use crate::ebx::{Link, Value};
use crate::{Alpha, Error};

/// Parameter names that hold the colour map, most specific first.
const DIFFUSE: [&str; 5] = ["Diffuse", "Di", "_Diffuse", "diffuse", "Texture"];

/// For each mesh asset (by ebx name), the diffuse texture asset name per material slot.
/// Where levels disagree, the base variation (hash 0) with the most bindings wins.
pub fn diffuse_maps(alpha: &Alpha) -> Result<BTreeMap<String, Vec<Option<String>>>, Error> {
    // Partition GUID -> ebx name, to resolve the databases' external links.
    let mut names = HashMap::new();
    let mut databases = Vec::new();
    for name in alpha.ebx.keys() {
        let p = alpha.asset(name)?;
        names.insert(p.file_guid, name.clone());
        if name.ends_with("/meshvariationdb_win32") {
            databases.push(p);
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
    Ok(out)
}
