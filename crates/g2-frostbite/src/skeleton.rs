//! `SkeletonAsset`s: a unit's bones by name, each bone's parent, and its bind pose in model
//! space (`ModelPose`: a `LinearTransform` of right/up/forward axes and a translation). A
//! skinned mesh (`<unit>_skn_mesh`) has no link to its skeleton; the blueprints that draw it
//! name one through a `MasterSkeletonAsset` (`_mskl`), and its subsets' bone palettes index it.
//! Infantry share one: every soldier's blueprint points at the EU rifle soldier's.

use std::collections::{BTreeMap, HashMap};

use crate::ebx::{Link, Value};
use crate::{Alpha, Error};

pub struct Skeleton {
    pub names: Vec<String>,
    /// Each bone's parent index; `None` for the root.
    pub parents: Vec<Option<usize>>,
    /// Bind pose per bone in model space, column-major 4x4 (Frostbite's handedness).
    pub model_pose: Vec<[f32; 16]>,
}

impl Skeleton {
    pub fn read(alpha: &Alpha, name: &str) -> Result<Skeleton, Error> {
        let p = alpha.asset(name)?;
        let prim = p.primary().filter(|v| v.ty() == Some("SkeletonAsset")).ok_or(Error::Bad("not a SkeletonAsset"))?;
        let names: Vec<String> = prim
            .get("BoneNames")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::str)
            .map(str::to_owned)
            .collect();
        let parents: Vec<Option<usize>> = prim
            .get("Hierarchy")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .map(|v| v.int().and_then(|i| usize::try_from(i).ok()))
            .collect();
        let model_pose: Vec<[f32; 16]> = prim
            .get("ModelPose")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .map(transform)
            .collect::<Result<_, _>>()?;
        if parents.len() != names.len() || model_pose.len() != names.len() {
            return Err(Error::Bad("skeleton arrays differ in length"));
        }
        if parents.iter().enumerate().any(|(i, p)| p.is_some_and(|p| p >= i)) {
            return Err(Error::Bad("skeleton bone listed before its parent"));
        }
        Ok(Skeleton { names, parents, model_pose })
    }

    /// The skeleton for a skinned mesh: the one a blueprint's animation component pairs it
    /// with (`links`, from [`Skeleton::links`]), else `<mesh name>_skl` beside it, else the one
    /// `_skl` asset in the mesh's folder, if exactly one.
    pub fn for_mesh<'a>(alpha: &'a Alpha, links: &'a BTreeMap<String, String>, mesh: &str) -> Option<&'a String> {
        if let Some(s) = links.get(mesh) {
            return Some(s);
        }
        let stem = mesh.strip_suffix("_skn_mesh").or_else(|| mesh.strip_suffix("_mesh"))?;
        if let Some((name, _)) = alpha.ebx.get_key_value(&format!("{stem}_skl")) {
            return Some(name);
        }
        let folder = mesh.rsplit_once('/')?.0;
        let mut found =
            alpha.ebx.keys().filter(|n| n.ends_with("_skl") && n.rsplit_once('/').is_some_and(|(f, _)| f == folder));
        let first = found.next()?;
        found.next().is_none().then_some(first)
    }

    /// Mesh → skeleton, as the blueprints pair them: an `AntAnimatableData`'s
    /// `MasterSkeletonAsset` and the meshes of the same component, the master skeleton's
    /// `MasterSkeleton` being the `SkeletonAsset`. A blueprint whose component lists no
    /// meshes (the APA rocket soldier's) pairs its first master skeleton with every skinned
    /// mesh it imports. Where blueprints disagree, the first by name wins. An unreadable asset
    /// is skipped (the import reports it elsewhere).
    pub fn links(alpha: &Alpha) -> BTreeMap<String, String> {
        let parsed: Vec<(&String, crate::ebx::Partition)> =
            alpha.ebx.keys().filter_map(|n| alpha.asset(n).ok().map(|p| (n, p))).collect();
        let names: HashMap<[u8; 16], &String> = parsed.iter().map(|(n, p)| (p.file_guid, *n)).collect();
        let skeleton_of = |mskl: &str| -> Option<String> {
            let (_, p) = parsed.iter().find(|(n, _)| n.as_str() == mskl)?;
            match p.primary()?.get("MasterSkeleton")?.link()? {
                Link::External(i) => names.get(&p.imports.get(i)?.0).map(|s| (*s).clone()),
                _ => None,
            }
        };
        let mut out = BTreeMap::new();
        for (_, p) in &parsed {
            let external = |v: Option<&Value>| match v.and_then(Value::link) {
                Some(Link::External(i)) => p.imports.get(i).and_then(|(f, _)| names.get(f)).map(|s| s.as_str()),
                _ => None,
            };
            let mut first = None;
            for inst in &p.instances {
                let mut data = Vec::new();
                find(&inst.value, "AntAnimatableData", &mut data);
                let Some(mskl) = data.first().and_then(|d| external(d.get("MasterSkeletonAsset"))) else { continue };
                let Some(skl) = skeleton_of(mskl) else { continue };
                let mut meshes = Vec::new();
                find(&inst.value, "AntAnimatableComponentMeshData", &mut meshes);
                for m in meshes.iter().filter_map(|m| external(m.get("Mesh"))) {
                    out.entry(m.to_owned()).or_insert_with(|| skl.clone());
                }
                first.get_or_insert(skl);
            }
            let Some(skl) = first else { continue };
            for (file, _) in &p.imports {
                if let Some(m) = names.get(file).filter(|m| m.ends_with("_skn_mesh")) {
                    out.entry((*m).clone()).or_insert_with(|| skl.clone());
                }
            }
        }
        out
    }
}

/// Every value of class `ty` inside `v`.
fn find<'a>(v: &'a Value, ty: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Complex { ty: t, fields } => {
            if t == ty {
                out.push(v);
            }
            fields.iter().for_each(|(_, f)| find(f, ty, out));
        }
        Value::Array(a) => a.iter().for_each(|x| find(x, ty, out)),
        _ => {}
    }
}

/// A `LinearTransform` as a column-major 4x4: right, up, forward, then the translation.
fn transform(v: &Value) -> Result<[f32; 16], Error> {
    let axis = |key: &str| -> Result<[f32; 3], Error> {
        let a = v.get(key).ok_or(Error::Bad("transform without an axis"))?;
        let c = |k: &str| match a.get(k) {
            Some(Value::Float(f)) => Ok(*f as f32),
            _ => Err(Error::Bad("transform axis component")),
        };
        Ok([c("x")?, c("y")?, c("z")?])
    };
    let [r, u, f, t] = [axis("right")?, axis("up")?, axis("forward")?, axis("trans")?];
    Ok([r[0], r[1], r[2], 0.0, u[0], u[1], u[2], 0.0, f[0], f[1], f[2], 0.0, t[0], t[1], t[2], 1.0])
}
