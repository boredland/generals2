//! Reader for the 2013 alpha's UTFWin layouts (`UTFWinAssets/layouts/*.layout`, imported by
//! `g2-tools alpha DIR import`): XML trees of windows, each with an area on a 1600×1200
//! canvas, an optional image, text, and "WinProcs" that say how it scales. Images are named
//! by a 32-bit FNV-1 hash of their lowercased file stem, found under `images/`.
//!
//! Scaling as the alpha draws it (measured against its gameplay footage at 1920×1080): a
//! window whose `RtsSimpleScalingLayout` preserves aspect ratio is scaled uniformly by
//! min(width/1600, height/1200) and anchored by its horizontal and vertical bias (0 start,
//! 1 centre, 2 end); every window inside it keeps its place relative to that anchor.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The canvas the layouts are authored on.
pub const CANVAS: (f32, f32) = (1600.0, 1200.0);

#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// `Comment` (the designers' name for it), or empty.
    pub name: String,
    pub class: String,
    /// x0, y0, x1, y1, relative to the parent.
    pub area: [f32; 4],
    /// An image fill: path relative to the `images/` folder.
    pub image: Option<PathBuf>,
    /// Image repeats instead of stretching.
    pub tiled: bool,
    /// A solid fill colour (ARGB) when its alpha is non-zero.
    pub fill: Option<u32>,
    /// Anchoring of a window that keeps its aspect ratio: (horizontal, vertical) bias.
    pub anchor: Option<(u8, u8)>,
    /// A text window's colour (ARGB) and text-style id (`TextStyles.css`).
    pub text: Option<(u32, u32)>,
    pub children: Vec<Window>,
}

impl Window {
    /// The first window, depth first, with this name.
    pub fn find(&self, name: &str) -> Option<&Window> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(name))
    }
}

/// FNV-1 (multiply then xor), 32-bit: the image key of a lowercased file stem.
pub fn fnv1(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5, |h, b| h.wrapping_mul(0x0100_0193) ^ u32::from(b))
}

/// Image key → path under `images/`, for every PNG there.
pub fn image_index(images: &Path) -> HashMap<u32, PathBuf> {
    let mut out = HashMap::new();
    let mut stack = vec![images.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("png"))
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                out.insert(fnv1(&stem.to_ascii_lowercase()), path.strip_prefix(images).unwrap().to_path_buf());
            }
        }
    }
    out
}

pub fn parse(xml: &str, images: &HashMap<u32, PathBuf>) -> Result<Window, String> {
    let doc = roxmltree::Document::parse(xml.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
    let root = doc.root_element().children().find(|n| n.has_tag_name("object")).ok_or("layout without a window")?;
    Ok(window(root, images))
}

fn props<'a, 'i>(node: roxmltree::Node<'a, 'i>) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    node.children().filter(|c| c.has_tag_name("prop"))
}

fn prop<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    props(node).find(|p| p.attribute("name") == Some(name))
}

fn hex(v: &str) -> Option<u32> {
    match v.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16).ok(),
        None => v.parse().ok(),
    }
}

fn window(node: roxmltree::Node, images: &HashMap<u32, PathBuf>) -> Window {
    let value = |name: &str| prop(node, name).and_then(|p| p.attribute("value"));
    let area = value("Area")
        .map(|v| {
            let n: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if n.len() == 4 { [n[0], n[1], n[2], n[3]] } else { [0.0; 4] }
        })
        .unwrap_or([0.0; 4]);
    // Designers spelled it both ways.
    let name = value("Comment").or_else(|| value("Commment")).unwrap_or_default().to_owned();
    let objects =
        |name: &str| prop(node, name).into_iter().flat_map(|p| p.children().filter(|c| c.has_tag_name("object")));
    let mut image = None;
    let mut tiled = false;
    for d in objects("FillDrawable").chain(objects("WinProcs")) {
        let Some(key) = prop(d, "Image").and_then(|p| p.attribute("key")) else { continue };
        let k = key.rsplit(':').next().and_then(|k| u64::from_str_radix(k, 16).ok()).map(|k| k as u32);
        if let Some(path) = k.and_then(|k| images.get(&k)) {
            image = Some(path.clone());
            tiled = prop(d, "Tiling").and_then(|p| p.attribute("value")) == Some("1");
            break;
        }
    }
    let fill = value("FillColor").and_then(hex).filter(|c| c >> 24 != 0);
    let anchor = objects("WinProcs").find_map(|p| {
        let v = |n: &str| prop(p, n).and_then(|x| x.attribute("value"));
        (matches!(p.attribute("cls"), Some("RtsSimpleScalingLayout" | "RtsScalingDynamicWindowLayout"))
            && v("Preserve aspect ratio") == Some("True"))
        .then(|| {
            let bias = |n: &str| v(n).and_then(|x| x.parse().ok()).unwrap_or(0);
            (bias("Horizontal aspect ratio bias"), bias("Vertical aspect ratio bias"))
        })
    });
    let class = node.attribute("cls").unwrap_or_default().to_owned();
    let text = matches!(class.as_str(), "Text" | "Localized Text").then(|| {
        (value("TextColor").and_then(hex).unwrap_or(0xffff_ffff), value("TextFont").and_then(hex).unwrap_or(1))
    });
    Window {
        name,
        class,
        area,
        image,
        tiled,
        fill,
        anchor,
        text,
        children: objects("Children").map(|c| window(c, images)).collect(),
    }
}

/// Where a window lands on a `screen`-sized display: its anchored ancestor's rule, carried
/// down. `parent` is the parent's on-screen origin and the scale in force.
pub fn place(w: &Window, parent: (f32, f32), scale: f32, screen: (f32, f32)) -> ([f32; 4], f32) {
    let [x0, y0, x1, y1] = w.area;
    let (origin, s) = match w.anchor {
        Some((h, v)) => {
            let s = (screen.0 / CANVAS.0).min(screen.1 / CANVAS.1);
            let at = |bias: u8, pos: f32, canvas: f32, screen: f32| match bias {
                1 => screen / 2.0 + (pos - canvas / 2.0) * s,
                2 => screen - (canvas - pos) * s,
                _ => pos * s,
            };
            // The anchored window's area is in canvas space: place it from the screen edge.
            return ([at(h, x0, CANVAS.0, screen.0), at(v, y0, CANVAS.1, screen.1), (x1 - x0) * s, (y1 - y0) * s], s);
        }
        None => (parent, scale),
    };
    ([origin.0 + x0 * s, origin.1 + y0 * s, (x1 - x0) * s, (y1 - y0) * s], s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_keys_are_fnv1_of_the_lowercased_stem() {
        // From the alpha: `bottomBarUI.layout` names `BottomBarSkins/Divider1.png` by 09dfb00d.
        assert_eq!(fnv1("divider1"), 0x09df_b00d);
        assert_eq!(fnv1(""), 0x811c_9dc5);
    }

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<graph class="Layout" type="Layout">
  <object cls="Window">
    <prop name="Area" type="rectf" value="0,0,1600,1200" />
    <prop name="Comment" type="string" value="UI Container" />
    <prop name="Children" type="object" count="1">
      <object cls="Window">
        <prop name="Area" type="rectf" value="0,790,900,1200" />
        <prop name="Comment" type="string" value="Bottom left" />
        <prop name="WinProcs" type="object" count="1">
          <object cls="RtsSimpleScalingLayout">
            <prop name="Preserve aspect ratio" type="bool" value="True" />
            <prop name="Vertical aspect ratio bias" type="uint32" value="2" />
            <prop name="Horizontal aspect ratio bias" type="uint32" value="0" />
          </object>
        </prop>
        <prop name="Children" type="object" count="2">
          <object cls="Window">
            <prop name="Area" type="rectf" value="0,87,310,412" />
            <prop name="FillColor" type="uint32" value="0x00212121" />
            <prop name="Commment" type="string" value="Minimap" />
            <prop name="FillDrawable" type="object">
              <object cls="ImageDrawable">
                <prop name="Image" type="image" key="2f7d0004:4f5ee58d:0000000009dfb00d" />
                <prop name="Tiling" type="uint32" value="1" />
              </object>
            </prop>
          </object>
          <object cls="Localized Text">
            <prop name="Area" type="rectf" value="10,10,110,40" />
            <prop name="FillColor" type="uint32" value="0x89222222" />
            <prop name="TextFont" type="uint32" value="0x00000009" />
            <prop name="TextColor" type="uint32" value="0xff00fd77" />
          </object>
        </prop>
      </object>
    </prop>
  </object>
</graph>"#;

    #[test]
    fn windows_carry_area_image_fill_text_and_anchor() {
        let images = HashMap::from([(0x09df_b00d, PathBuf::from("BottomBarSkins/Divider1.png"))]);
        let root = parse(SAMPLE, &images).unwrap();
        let corner = root.find("Bottom left").unwrap();
        assert_eq!(corner.anchor, Some((0, 2)));
        let map = root.find("Minimap").unwrap();
        assert_eq!(map.area, [0.0, 87.0, 310.0, 412.0]);
        assert_eq!(map.image.as_deref(), Some(Path::new("BottomBarSkins/Divider1.png")));
        assert!(map.tiled);
        assert_eq!(map.fill, None, "a zero-alpha fill is no fill");
        let text = &corner.children[1];
        assert_eq!((text.text, text.fill), (Some((0xff00_fd77, 9)), Some(0x8922_2222)));
    }

    #[test]
    fn anchored_windows_scale_uniformly_from_their_edge() {
        let images = HashMap::new();
        let root = parse(SAMPLE, &images).unwrap();
        let corner = root.find("Bottom left").unwrap();
        // At 1920×1080 the scale is 0.9 (height-bound) and the corner hugs the bottom.
        let ([x, y, w, h], s) = place(corner, (0.0, 0.0), 1.0, (1920.0, 1080.0));
        assert_eq!(s, 0.9);
        assert_eq!((x, y.round(), w, h.round()), (0.0, 711.0, 810.0, 369.0));
        // The minimap inside follows: its right edge lands at 279 px, as in the alpha's
        // 1080p footage.
        let ([mx, my, mw, _], _) = place(&corner.children[0], (x, y), s, (1920.0, 1080.0));
        assert_eq!((mx, (my).round(), (mx + mw).round()), (0.0, 789.0, 279.0));
        // A right-anchored window keeps its distance from the right edge.
        let mut right = corner.clone();
        right.anchor = Some((2, 2));
        right.area = [915.0, 630.0, 1597.0, 1200.0];
        let ([rx, ..], _) = place(&right, (0.0, 0.0), 1.0, (1920.0, 1080.0));
        assert_eq!(rx.round(), (1920.0 - 685.0 * 0.9_f32).round());
    }
}
