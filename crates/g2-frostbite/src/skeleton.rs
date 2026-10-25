//! `SkeletonAsset`s: a unit's bones by name, each bone's parent, and its bind pose in model
//! space (`ModelPose`: a `LinearTransform` of right/up/forward axes and a translation). A
//! skinned mesh (`<unit>_skn_mesh`) has no link to its skeleton; the skeleton sits beside it
//! as `<unit folder>/<name>_skl`, and its subsets' bone palettes index it.

use crate::ebx::Value;
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

    /// The skeleton for a skinned mesh: the one `_skl` asset in the mesh's folder, if exactly one.
    pub fn for_mesh<'a>(alpha: &'a Alpha, mesh: &str) -> Option<&'a String> {
        let folder = mesh.rsplit_once('/')?.0;
        let mut found =
            alpha.ebx.keys().filter(|n| n.ends_with("_skl") && n.rsplit_once('/').is_some_and(|(f, _)| f == folder));
        let first = found.next()?;
        found.next().is_none().then_some(first)
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
