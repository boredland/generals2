//! The procedural HUD's art (docs/STYLE.md, HUD): bevelled metal plates, recessed wells,
//! button faces and class silhouettes, drawn into small images once at startup and stretched
//! over the panels as nine-slices. The shapes are ours (cut corners, a faction trim line inside
//! the bevel); nothing is traced from the alpha's HUD.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use g2_sim::defs::{Armour, BuildingRole, Def, PowerKind};
use g2_sim::{Class, Faction};

/// Plate, well and button images are this many pixels square; the slice border is the part of
/// each edge that keeps its size (bevel, trim and cut corner), the rest stretches.
const PLATE: u32 = 64;
pub(crate) const PLATE_EDGE: f32 = 14.0;
/// The match bar's plate: shorter than the bottom ones, so a smaller image keeps its bevel at
/// full size.
const BAR: u32 = 32;
pub(crate) const BAR_EDGE: f32 = 10.0;
const WELL: u32 = 24;
pub(crate) const WELL_EDGE: f32 = 8.0;
const FACE: u32 = 32;
pub(crate) const FACE_EDGE: f32 = 8.0;
const ICON: u32 = 64;

/// The trim colour of the local player's faction: the team palette's EU teal, APA red and GLA
/// sand (`models.rs`), lifted a little so a line one or two pixels wide reads on dark metal.
pub(crate) fn trim(f: Faction) -> Color {
    match f {
        Faction::Eu => super::TEAL,
        Faction::Apa => Color::srgb(0.90, 0.26, 0.18),
        Faction::Gla => Color::srgb(0.90, 0.72, 0.34),
    }
}

/// A button face's look, by state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Face {
    Idle,
    /// Under the cursor, or a support power that is ready.
    Hover,
    /// Held down, or the slot being placed or the power being aimed.
    Pressed,
    /// Nothing to do: an empty slot, one the player can't afford, a power not available.
    Off,
}

const FACES: [Face; 4] = [Face::Idle, Face::Hover, Face::Pressed, Face::Off];

/// Silhouettes for the selection portrait, the build buttons and the support powers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Icon {
    Infantry,
    Vehicle,
    Aircraft,
    Dozer,
    Collector,
    Hq,
    Factory,
    Power,
    Supply,
    Derrick,
    Lab,
    Defense,
    Missile,
    Building,
    Barrage,
    Repair,
    Cloak,
    Recon,
    Upgrade,
}

const ICONS: [Icon; 19] = [
    Icon::Infantry,
    Icon::Vehicle,
    Icon::Aircraft,
    Icon::Dozer,
    Icon::Collector,
    Icon::Hq,
    Icon::Factory,
    Icon::Power,
    Icon::Supply,
    Icon::Derrick,
    Icon::Lab,
    Icon::Defense,
    Icon::Missile,
    Icon::Building,
    Icon::Barrage,
    Icon::Repair,
    Icon::Cloak,
    Icon::Recon,
    Icon::Upgrade,
];

impl Icon {
    /// What a def is at a glance: infantry before collector (GLA workers gather on foot), then
    /// the vehicle's job, a building's role.
    pub(crate) fn of_def(def: &Def) -> Icon {
        match def.class {
            Class::Unit { .. } if def.armour == Armour::Infantry => Icon::Infantry,
            Class::Unit { collector: Some(_), .. } => Icon::Collector,
            Class::Unit { constructor: true, .. } => Icon::Dozer,
            Class::Unit { flies: true, .. } => Icon::Aircraft,
            Class::Unit { .. } => Icon::Vehicle,
            Class::Building { role, .. } => match role {
                BuildingRole::CommandCenter => Icon::Hq,
                BuildingRole::SupplyCenter { .. } => Icon::Supply,
                BuildingRole::PowerPlant => Icon::Power,
                BuildingRole::OilDerrick { .. } => Icon::Derrick,
                BuildingRole::Factory => Icon::Factory,
                BuildingRole::Lab => Icon::Lab,
                BuildingRole::Defense => Icon::Defense,
                BuildingRole::Superweapon => Icon::Missile,
                BuildingRole::Civilian | BuildingRole::Bunker => Icon::Building,
            },
            Class::SupplyPile => Icon::Supply,
            Class::OilWell => Icon::Derrick,
        }
    }

    pub(crate) fn of_power(kind: &PowerKind) -> Icon {
        match kind {
            PowerKind::Barrage { .. } => Icon::Barrage,
            PowerKind::Repair { .. } => Icon::Repair,
            PowerKind::Ambush { .. } => Icon::Infantry,
            PowerKind::Airstrike { .. } => Icon::Aircraft,
            PowerKind::Recon { .. } => Icon::Recon,
            PowerKind::Missiles { .. } => Icon::Missile,
            PowerKind::Cloak { .. } => Icon::Cloak,
            // A charge set in an enemy building: the building mark.
            PowerKind::Sabotage { .. } => Icon::Building,
        }
    }
}

/// One faction's frame set.
pub(crate) struct Kit {
    pub plate: Handle<Image>,
    pub bar: Handle<Image>,
    pub well: Handle<Image>,
    faces: Vec<Handle<Image>>,
}

impl Kit {
    pub(crate) fn face(&self, f: Face) -> Handle<Image> {
        self.faces[f as usize].clone()
    }
}

#[derive(Resource)]
pub(crate) struct HudArt {
    /// By `Faction as usize`.
    kits: Vec<Kit>,
    /// By `Icon as usize`.
    icons: Vec<Handle<Image>>,
}

impl HudArt {
    pub(crate) fn kit(&self, f: Faction) -> &Kit {
        &self.kits[f as usize]
    }

    pub(crate) fn icon(&self, i: Icon) -> Handle<Image> {
        self.icons[i as usize].clone()
    }
}

/// Draws every image once: three frame sets of seven images and the icons, about 130 KB in all.
pub(crate) fn make(mut commands: Commands, images: Option<ResMut<Assets<Image>>>) {
    let Some(mut images) = images else { return };
    let kits = [Faction::Eu, Faction::Apa, Faction::Gla]
        .map(|f| {
            let t = trim(f).to_srgba();
            let t = Vec3::new(t.red, t.green, t.blue);
            Kit {
                plate: images.add(plate(PLATE, [9.0, 9.0, 0.0, 0.0], true, t)),
                bar: images.add(plate(BAR, [0.0, 0.0, 6.0, 6.0], false, t)),
                well: images.add(well(t)),
                faces: FACES.iter().map(|&s| images.add(face(s, t))).collect(),
            }
        })
        .into();
    let icons = ICONS.iter().map(|&i| images.add(paint(ICON, |p| silhouette(shape(i, p), p)))).collect();
    commands.insert_resource(HudArt { kits, icons });
}

/// The node bundle for a nine-sliced frame image over the node's whole box (padding keeps
/// content clear of the frame), its fixed edges scaled with the HUD (`k`).
pub(crate) fn sliced(image: Handle<Image>, edge: f32, k: f32) -> ImageNode {
    ImageNode::new(image).with_visual_box(VisualBox::BorderBox).with_mode(NodeImageMode::Sliced(TextureSlicer {
        border: BorderRect::all(edge),
        center_scale_mode: SliceScaleMode::Stretch,
        sides_scale_mode: SliceScaleMode::Stretch,
        max_corner_scale: k,
    }))
}

fn paint(side: u32, f: impl Fn(Vec2) -> [f32; 4]) -> Image {
    let mut data = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let c = f(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
            data.extend(c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
    }
    let mut image = Image::new(
        Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = bevy::image::ImageSampler::linear();
    image
}

/// Gunmetal at brightness `v`, with the faint blue of the menus' and models' dark steel.
fn steel(v: f32) -> Vec3 {
    Vec3::new(v, v * 1.08, v * 1.17)
}

/// A fixed per-pixel hash in -1..1, for brushed-metal grain.
fn noise(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 32767.5 - 1.0
}

/// Distance from `p` inward to the nearest edge of a `side` square whose corners are cut by
/// `cut` (top-left, top-right, bottom-right, bottom-left; 0 keeps a corner square), and how
/// that edge faces the light from the upper left: 1 lit, -1 shaded, 0 side-on.
fn inset(p: Vec2, side: f32, cut: [f32; 4]) -> (f32, f32) {
    let (l, t, r, b) = (p.x, p.y, side - p.x, side - p.y);
    let s = std::f32::consts::FRAC_1_SQRT_2;
    [
        (t, 1.0),
        (l, 1.0),
        (r, -1.0),
        (b, -1.0),
        ((l + t - cut[0]) * s, 1.0),
        ((r + t - cut[1]) * s, 0.0),
        ((r + b - cut[2]) * s, -1.0),
        ((l + b - cut[3]) * s, 0.0),
    ]
    .into_iter()
    .fold((f32::MAX, 0.0), |m, e| if e.0 < m.0 { e } else { m })
}

/// A plate: brushed gunmetal darkening downward, a black rim, a two-pixel bevel lit from the
/// upper left, the trim line in a groove inside it, corners cut by `cut` and, with `rivets`, a
/// rivet in each corner. The bottom plates cut their top corners and stand on the screen's
/// bottom edge; the match bar cuts its bottom ones and hangs from the top.
fn plate(size: u32, cut: [f32; 4], rivets: bool, trim: Vec3) -> Image {
    let side = size as f32;
    let studs = [
        Vec2::new(12.0, 12.0),
        Vec2::new(side - 12.0, 12.0),
        Vec2::new(9.0, side - 9.0),
        Vec2::new(side - 9.0, side - 9.0),
    ];
    paint(size, |p| {
        let (d, light) = inset(p, side, cut);
        let grain = noise(0, p.y as u32) * 0.05 + noise(p.x as u32, p.y as u32) * 0.02;
        let fill = steel(0.155 - 0.07 * p.y / side) * (1.0 + grain);
        let mut c = match d {
            d if d < 1.0 => steel(0.025),
            d if d < 3.0 => fill + Vec3::splat(light * 0.09 + 0.03),
            d if d < 4.0 => steel(0.04),
            d if d < 5.5 => trim,
            d if d < 6.5 => steel(0.04),
            _ => fill,
        };
        for r in studs.into_iter().filter(|_| rivets) {
            let v = p - r;
            if v.length() < 1.9 {
                c = if v.x + v.y < 0.0 { steel(0.46) } else { steel(0.20) };
            } else if (v - Vec2::splat(0.9)).length() < 2.3 {
                c = steel(0.03);
            }
        }
        [c.x, c.y, c.z, (d + 0.5).clamp(0.0, 1.0) * 0.96]
    })
}

/// A recess the minimap and the portrait sit in: the rim lit from below as a hole is, the trim
/// line, a dark lip, and a clear centre for what it holds.
fn well(trim: Vec3) -> Image {
    let side = WELL as f32;
    paint(WELL, |p| {
        let (d, light) = inset(p, side, [4.0; 4]);
        let (c, a) = match d {
            d if d < 1.0 => (steel(0.2 - light * 0.17), 1.0),
            d if d < 2.2 => (trim * 0.85, 1.0),
            d if d < 5.0 => (steel(0.02), 1.0),
            _ => (Vec3::ZERO, 0.0),
        };
        [c.x, c.y, c.z, a * (d + 0.5).clamp(0.0, 1.0)]
    })
}

/// A button face: the top-left and bottom-right corners cut, a one-pixel bevel and a vertical
/// gradient. Hover lifts it and lights the rim in the trim colour; pressed sinks it (bevel and
/// gradient inverted); off is flat and dim.
fn face(state: Face, trim: Vec3) -> Image {
    let side = FACE as f32;
    paint(FACE, |p| {
        let (d, light) = inset(p, side, [5.0, 0.0, 5.0, 0.0]);
        let v = p.y / side;
        let (rim, bevel, top, bottom, alpha) = match state {
            Face::Idle => (steel(0.02), light, 0.22, 0.12, 0.96),
            Face::Hover => (trim, light * 1.3, 0.28, 0.15, 0.97),
            Face::Pressed => (trim, -light, 0.08, 0.15, 0.97),
            Face::Off => (steel(0.14), 0.0, 0.085, 0.07, 0.88),
        };
        let fill = steel(top + (bottom - top) * v);
        let c = match d {
            d if d < 1.0 => rim,
            d if d < 2.0 => fill + Vec3::splat(bevel * 0.13),
            // Hover and pressed carry a faint inner trim glow.
            d if d < 3.0 && matches!(state, Face::Hover | Face::Pressed) => fill.lerp(trim, 0.35),
            _ => fill,
        };
        [c.x, c.y, c.z, alpha * (d + 0.5).clamp(0.0, 1.0)]
    })
}

/// A light silhouette, shaded darker toward its foot, with a soft dark outline, from a signed
/// distance (negative inside) in the icon's 64-pixel space.
fn silhouette(d: f32, p: Vec2) -> [f32; 4] {
    let fill = (0.5 - d).clamp(0.0, 1.0);
    let a = fill.max((2.2 - d).clamp(0.0, 1.0) * 0.8);
    let shade = 1.0 - 0.3 * p.y / ICON as f32;
    let c = if a > 0.0 { shade * fill / a } else { 0.0 };
    [c, c, c, a]
}

fn circle(p: Vec2, c: Vec2, r: f32) -> f32 {
    p.distance(c) - r
}

/// A box of half-size `h` about `c` with corners rounded by `r`.
fn rect(p: Vec2, c: Vec2, h: Vec2, r: f32) -> f32 {
    let q = (p - c).abs() - h + Vec2::splat(r);
    q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - r
}

/// A capsule from `a` to `b`, radius `r`.
fn seg(p: Vec2, a: Vec2, b: Vec2, r: f32) -> f32 {
    let (pa, ba) = (p - a, b - a);
    let h = (pa.dot(ba) / ba.dot(ba)).clamp(0.0, 1.0);
    (pa - ba * h).length() - r
}

/// Any simple polygon (Inigo Quilez's exact polygon distance).
fn poly(p: Vec2, v: &[Vec2]) -> f32 {
    let mut d = (p - v[0]).length_squared();
    let mut s = 1.0;
    let mut j = v.len() - 1;
    for i in 0..v.len() {
        let (e, w) = (v[j] - v[i], p - v[i]);
        let b = w - e * (w.dot(e) / e.dot(e)).clamp(0.0, 1.0);
        d = d.min(b.length_squared());
        let c = [p.y >= v[i].y, p.y < v[j].y, e.x * w.y > e.y * w.x];
        if c.iter().all(|&c| c) || c.iter().all(|&c| !c) {
            s = -s;
        }
        j = i;
    }
    s * d.sqrt()
}

fn union(ds: &[f32]) -> f32 {
    ds.iter().copied().fold(f32::MAX, f32::min)
}

/// `a` with `b` cut out of it.
fn minus(a: f32, b: f32) -> f32 {
    a.max(-b)
}

/// The icon shapes, in a 64-pixel square, y down, standing on about y = 56.
fn shape(icon: Icon, p: Vec2) -> f32 {
    let v = Vec2::new;
    match icon {
        Icon::Infantry => union(&[
            circle(p, v(30.0, 13.0), 6.0),
            rect(p, v(30.0, 29.0), v(7.0, 10.0), 3.0),
            seg(p, v(27.0, 38.0), v(23.0, 55.0), 3.5),
            seg(p, v(33.0, 38.0), v(38.0, 55.0), 3.5),
            seg(p, v(18.0, 27.0), v(52.0, 21.0), 1.8),
            seg(p, v(30.0, 23.0), v(42.0, 26.0), 2.8),
        ]),
        Icon::Vehicle => {
            let wheels = union(&[14.0, 24.0, 34.0, 44.0, 54.0].map(|x| circle(p, v(x, 48.0), 2.4)));
            union(&[
                minus(rect(p, v(32.0, 48.0), v(26.0, 6.0), 5.5), wheels),
                poly(p, &[v(9.0, 42.0), v(55.0, 42.0), v(51.0, 34.0), v(15.0, 34.0)]),
                rect(p, v(28.0, 29.0), v(10.0, 5.5), 3.0),
                seg(p, v(36.0, 28.0), v(60.0, 26.0), 1.8),
            ])
        }
        Icon::Aircraft => union(&[
            poly(p, &[v(32.0, 4.0), v(36.0, 14.0), v(36.0, 50.0), v(32.0, 58.0), v(28.0, 50.0), v(28.0, 14.0)]),
            poly(p, &[v(32.0, 20.0), v(60.0, 42.0), v(60.0, 46.0), v(32.0, 38.0), v(4.0, 46.0), v(4.0, 42.0)]),
            poly(p, &[v(32.0, 46.0), v(44.0, 57.0), v(44.0, 60.0), v(32.0, 56.0), v(20.0, 60.0), v(20.0, 57.0)]),
        ]),
        Icon::Dozer => union(&[
            rect(p, v(36.0, 49.0), v(18.0, 5.5), 5.0),
            rect(p, v(37.0, 38.0), v(15.0, 7.0), 2.0),
            minus(rect(p, v(42.0, 25.0), v(8.0, 7.0), 1.5), rect(p, v(43.0, 24.0), v(5.0, 3.5), 0.5)),
            poly(p, &[v(4.0, 30.0), v(12.0, 32.0), v(12.0, 55.0), v(4.0, 55.0)]),
            seg(p, v(12.0, 44.0), v(24.0, 40.0), 2.2),
            seg(p, v(30.0, 31.0), v(30.0, 20.0), 1.5),
        ]),
        Icon::Collector => {
            let hubs = union(&[14.0, 32.0, 51.0].map(|x| circle(p, v(x, 48.0), 2.0)));
            union(&[
                minus(
                    poly(p, &[v(42.0, 24.0), v(54.0, 24.0), v(60.0, 34.0), v(60.0, 44.0), v(42.0, 44.0)]),
                    poly(p, &[v(46.0, 27.0), v(53.0, 27.0), v(57.0, 34.0), v(46.0, 34.0)]),
                ),
                rect(p, v(23.0, 40.0), v(18.0, 4.0), 1.0),
                rect(p, v(14.0, 30.0), v(7.0, 6.0), 0.8),
                rect(p, v(30.0, 30.0), v(7.0, 6.0), 0.8),
                rect(p, v(22.0, 19.0), v(7.0, 5.0), 0.8),
                minus(union(&[14.0, 32.0, 51.0].map(|x| circle(p, v(x, 48.0), 5.5))), hubs),
            ])
        }
        Icon::Hq => {
            let windows = union(&[16.0, 26.0, 38.0, 48.0].map(|x| rect(p, v(x, 46.0), v(2.5, 2.5), 0.0)));
            union(&[
                minus(rect(p, v(32.0, 46.0), v(26.0, 8.0), 1.0), windows),
                rect(p, v(32.0, 34.0), v(18.0, 6.0), 1.0),
                rect(p, v(30.0, 24.0), v(9.0, 5.0), 1.0),
                seg(p, v(40.0, 24.0), v(40.0, 6.0), 1.4),
                minus(circle(p, v(22.0, 14.0), 6.0), circle(p, v(25.0, 11.0), 6.0)),
                seg(p, v(22.0, 14.0), v(26.0, 20.0), 1.2),
            ])
        }
        Icon::Factory => union(&[
            minus(rect(p, v(30.0, 44.0), v(24.0, 10.0), 1.0), rect(p, v(26.0, 49.0), v(8.0, 6.0), 0.0)),
            poly(
                p,
                &[
                    v(6.0, 34.0),
                    v(6.0, 26.0),
                    v(18.0, 18.0),
                    v(18.0, 26.0),
                    v(30.0, 18.0),
                    v(30.0, 26.0),
                    v(42.0, 18.0),
                    v(42.0, 26.0),
                    v(54.0, 26.0),
                    v(54.0, 34.0),
                ],
            ),
            rect(p, v(50.0, 16.0), v(3.5, 12.0), 0.5),
        ]),
        Icon::Power => {
            let tower = |x: f32| {
                poly(
                    p,
                    &[
                        v(x - 10.0, 54.0),
                        v(x - 6.0, 36.0),
                        v(x - 7.0, 22.0),
                        v(x + 7.0, 22.0),
                        v(x + 6.0, 36.0),
                        v(x + 10.0, 54.0),
                    ],
                )
            };
            union(&[tower(20.0), tower(44.0), circle(p, v(22.0, 13.0), 5.0), circle(p, v(30.0, 9.0), 4.0)])
        }
        Icon::Supply => {
            let crate_at =
                |x: f32, y: f32| minus(rect(p, v(x, y), v(11.0, 8.0), 1.0), rect(p, v(x, y), v(1.0, 8.0), 0.0));
            union(&[crate_at(19.0, 46.0), crate_at(43.0, 46.0), crate_at(31.0, 29.0)])
        }
        Icon::Derrick => union(&[
            rect(p, v(32.0, 55.0), v(26.0, 3.0), 0.5),
            seg(p, v(24.0, 54.0), v(34.0, 22.0), 2.0),
            seg(p, v(44.0, 54.0), v(34.0, 22.0), 2.0),
            seg(p, v(10.0, 26.0), v(56.0, 18.0), 2.8),
            poly(p, &[v(4.0, 22.0), v(14.0, 20.0), v(16.0, 36.0), v(8.0, 38.0)]),
            circle(p, v(52.0, 30.0), 5.0),
            seg(p, v(9.0, 36.0), v(9.0, 54.0), 1.2),
        ]),
        Icon::Lab => union(&[
            circle(p, v(32.0, 42.0), 17.0).max(p.y - 42.0),
            rect(p, v(32.0, 47.0), v(24.0, 5.0), 1.0),
            seg(p, v(40.0, 28.0), v(50.0, 8.0), 1.4),
            circle(p, v(50.0, 8.0), 3.0),
        ]),
        Icon::Defense => union(&[
            poly(p, &[v(12.0, 56.0), v(52.0, 56.0), v(46.0, 42.0), v(18.0, 42.0)]),
            circle(p, v(32.0, 36.0), 10.0),
            seg(p, v(38.0, 32.0), v(60.0, 20.0), 2.6),
        ]),
        Icon::Missile => union(&[
            poly(p, &[v(32.0, 3.0), v(38.0, 14.0), v(38.0, 44.0), v(26.0, 44.0), v(26.0, 14.0)]),
            poly(p, &[v(26.0, 32.0), v(17.0, 48.0), v(26.0, 45.0)]),
            poly(p, &[v(38.0, 32.0), v(47.0, 48.0), v(38.0, 45.0)]),
            rect(p, v(32.0, 55.0), v(22.0, 3.0), 0.5),
        ]),
        Icon::Building => {
            let holes = union(&[
                rect(p, v(32.0, 51.0), v(4.0, 6.0), 0.0),
                rect(p, v(20.0, 42.0), v(3.5, 3.5), 0.0),
                rect(p, v(44.0, 42.0), v(3.5, 3.5), 0.0),
            ]);
            union(&[
                minus(rect(p, v(32.0, 44.0), v(20.0, 12.0), 0.5), holes),
                poly(p, &[v(6.0, 32.0), v(32.0, 10.0), v(58.0, 32.0)]),
            ])
        }
        Icon::Barrage => {
            let shell = |x: f32, y: f32| {
                poly(p, &[v(x, y + 30.0), v(x + 5.0, y + 22.0), v(x + 5.0, y), v(x - 5.0, y), v(x - 5.0, y + 22.0)])
            };
            union(&[shell(16.0, 16.0), shell(32.0, 6.0), shell(48.0, 16.0)])
        }
        Icon::Repair => union(&[rect(p, v(32.0, 32.0), v(7.0, 22.0), 2.0), rect(p, v(32.0, 32.0), v(22.0, 7.0), 2.0)]),
        Icon::Cloak | Icon::Recon => {
            let lens = circle(p, v(32.0, 64.0), 40.0).max(circle(p, v(32.0, 0.0), 40.0));
            let eye = union(&[minus(lens, circle(p, v(32.0, 32.0), 8.0)), circle(p, v(32.0, 32.0), 4.5)]);
            if icon == Icon::Cloak {
                union(&[
                    minus(eye, seg(p, v(14.0, 50.0), v(50.0, 14.0), 4.5)),
                    seg(p, v(14.0, 50.0), v(50.0, 14.0), 2.2),
                ])
            } else {
                eye
            }
        }
        Icon::Upgrade => {
            let chevron = |y: f32| {
                poly(
                    p,
                    &[
                        v(32.0, y),
                        v(54.0, y + 22.0),
                        v(44.0, y + 22.0),
                        v(32.0, y + 10.0),
                        v(20.0, y + 22.0),
                        v(10.0, y + 22.0),
                    ],
                )
            };
            union(&[chevron(8.0), chevron(28.0)])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each silhouette covers a fair part of its square (a shape that drew nothing, or filled
    /// the square, would be a broken distance function) and no two look alike.
    #[test]
    fn icons_are_drawn_and_distinct() {
        let masks: Vec<Vec<bool>> = ICONS
            .iter()
            .map(|&i| {
                (0..ICON * ICON)
                    .map(|n| Vec2::new((n % ICON) as f32 + 0.5, (n / ICON) as f32 + 0.5))
                    .map(|p| shape(i, p) < 0.0)
                    .collect()
            })
            .collect();
        for (i, m) in ICONS.iter().zip(&masks) {
            let cover = m.iter().filter(|&&b| b).count() as f32 / (ICON * ICON) as f32;
            assert!((0.05..0.6).contains(&cover), "{i:?} covers {cover:.2} of its square");
        }
        for a in 0..masks.len() {
            for b in a + 1..masks.len() {
                let differ = masks[a].iter().zip(&masks[b]).filter(|(x, y)| x != y).count();
                assert!(differ > 150, "{:?} and {:?} differ in only {differ} pixels", ICONS[a], ICONS[b]);
            }
        }
    }

    /// A plate's corners are cut where they should be: transparent at the top corners, solid
    /// at the bottom ones (the plate stands on the screen's edge) and in the middle.
    #[test]
    fn plates_cut_their_top_corners() {
        let img = plate(PLATE, [9.0, 9.0, 0.0, 0.0], true, Vec3::ONE);
        let alpha = |x: u32, y: u32| img.data.as_ref().unwrap()[((y * PLATE + x) * 4 + 3) as usize];
        assert_eq!((alpha(0, 0), alpha(PLATE - 1, 0)), (0, 0));
        assert!(alpha(0, PLATE - 1) > 200 && alpha(PLATE - 1, PLATE - 1) > 200);
        assert!(alpha(PLATE / 2, PLATE / 2) > 200);
    }
}
