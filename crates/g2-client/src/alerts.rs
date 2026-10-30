//! Under-attack alerts, after OpenRA's `DamageNotifier` and `HarvesterAttackNotifier`: when an
//! enemy damages something of the local player's, a line at the top of the screen ("Our base
//! is under attack"), a ping on the minimap where it happened and a warning cue, at most once
//! per `interval` of each kind. V centres the camera on the latest one (OpenRA's
//! `ToLastEvent`, which is Space there; Space pauses here). Own entities are always known to
//! their player, so the fog never hides an alert. Client-side only: the alerts are read from
//! the sim's `Event::Hit`s, gathered every step in `sim::step_sim`.

use bevy::prelude::*;
use g2_sim::world::Event;
use g2_sim::{PlayerId, TICKS_PER_SECOND};

use crate::camera::RtsCamera;
use crate::sim::Sim;

/// What came under attack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Base,
    Unit,
    Collector,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::Base, Kind::Unit, Kind::Collector];

    /// OpenRA's `NotifyInterval`s: 30 s for the base (`DamageNotifier`) and collectors
    /// (`HarvesterAttackNotifier`), 10 s for other units (`DamageNotifier@Unit`). Counted on
    /// the match clock, so a paused match doesn't use them up.
    pub fn interval(self) -> u32 {
        match self {
            Kind::Base | Kind::Collector => 30 * TICKS_PER_SECOND,
            Kind::Unit => 10 * TICKS_PER_SECOND,
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Kind::Base => "Our base is under attack",
            Kind::Unit => "Our unit is under attack",
            Kind::Collector => "Our collector is under attack",
        }
    }
}

/// OpenRA shows a transient line for 4 s (`DisplayDurationMs: 4000`)...
const TEXT_SECS: f32 = 4.0;
/// ...and a radar ping for 250 of its 40 ms ticks (`RadarPingDuration`).
const PING_SECS: f32 = 10.0;
/// The ping closes in from a wide ring for its first 46 ticks (OpenRA's `FromRadius` 200 down
/// to `ToRadius` 15 at `ShrinkSpeed` 4 a tick), then pulses where it landed.
const PING_SHRINK_SECS: f32 = 1.85;

#[derive(Clone, Copy, Debug)]
pub struct Notice {
    pub kind: Kind,
    pub at: g2_sim::Vec2,
    pub tick: u32,
}

/// The throttle per kind, the notices not yet shown, what is on screen, and where the
/// latest alert was.
#[derive(Resource, Default)]
pub struct Alerts {
    /// The tick from which each kind may notify again.
    next: [u32; 3],
    /// New notices since `show` last drained them.
    pub fresh: Vec<Notice>,
    /// The line at the top and its age in seconds.
    pub line: Option<(Kind, f32)>,
    /// Minimap pings: where, and their age in seconds.
    pub pings: Vec<(g2_sim::Vec2, f32)>,
    /// Where V goes.
    pub last: Option<g2_sim::Vec2>,
}

impl Alerts {
    /// Takes the latest step's hits on `me`'s entities by another player, one notice per kind
    /// whose interval has passed.
    pub fn observe(&mut self, world: &g2_sim::World, me: PlayerId) {
        let tick = world.tick();
        for e in world.events() {
            let Event::Hit { at, target_def, by, victim, damage, .. } = *e else { continue };
            if victim != Some(me) || by == me || damage <= 0 {
                continue;
            }
            let def = world.rules.def(target_def);
            let kind = if def.is_building() {
                Kind::Base
            } else if def.collector().is_some() {
                Kind::Collector
            } else {
                Kind::Unit
            };
            let slot = Kind::ALL.iter().position(|&k| k == kind).unwrap();
            if tick >= self.next[slot] {
                self.next[slot] = tick + kind.interval();
                self.fresh.push(Notice { kind, at, tick });
            }
        }
    }
}

/// The alert line and the minimap pings, rebuilt every frame like the HUD's overlays.
#[derive(Component)]
struct Drawn;

pub struct AlertsPlugin;

// A new match gets fresh `Alerts` from `menu::swap_world`.
impl Plugin for AlertsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, show.after(crate::menu::MenuSet))
            .add_systems(Update, jump.in_set(crate::input::InputSet).run_if(crate::menu::playing));
    }
}

/// V: the camera to the latest alert.
fn jump(keys: Res<ButtonInput<KeyCode>>, sim: Res<Sim>, alerts: Res<Alerts>, mut cam: Query<&mut RtsCamera>) {
    if keys.just_pressed(KeyCode::KeyV)
        && let (Some(at), Ok(mut cam)) = (alerts.last, cam.single_mut())
    {
        let p = sim.to_world(at);
        cam.pivot = Vec3::new(p.x, cam.pivot.y, p.z);
    }
}

/// Turns fresh notices into the line, a ping and a cue, ages them (not while effects are
/// frozen for a screenshot), and redraws them: the line under the match bar, fading over its
/// last second, and each ping as a red ring on the minimap.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    sim: Res<Sim>,
    time: Res<Time>,
    frozen: Option<Res<crate::view::FreezeEffects>>,
    mut alerts: ResMut<Alerts>,
    heard: Option<ResMut<crate::sim::Heard>>,
    minimap: Query<(Entity, &ComputedNode, &Node), With<crate::hud::MinimapView>>,
    old: Query<Entity, With<Drawn>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let now = sim.world.tick();
    let fresh = core::mem::take(&mut alerts.fresh);
    if let Some(mut heard) = heard
        && !fresh.is_empty()
    {
        heard.0.push((crate::audio::Cue::Alert, None));
    }
    for n in fresh {
        // Notices gathered while no frame ran (a screenshot's warm-up) start as old as they are.
        let age = now.saturating_sub(n.tick) as f32 / TICKS_PER_SECOND as f32;
        alerts.line = Some((n.kind, age));
        alerts.pings.push((n.at, age));
        alerts.last = Some(n.at);
    }
    let dt = if frozen.is_some() { 0.0 } else { time.delta_secs() };
    if let Some((_, age)) = &mut alerts.line {
        *age += dt;
    }
    alerts.line = alerts.line.filter(|&(_, age)| age < TEXT_SECS);
    for (_, age) in &mut alerts.pings {
        *age += dt;
    }
    alerts.pings.retain(|&(_, age)| age < PING_SECS);

    if let Some((kind, age)) = alerts.line {
        let alpha = (TEXT_SECS - age).clamp(0.0, 1.0);
        let mut row = crate::hud::abs(percent(50), auto(), px(36), auto());
        row.margin = UiRect::left(px(-200));
        row.width = px(400);
        row.justify_content = JustifyContent::Center;
        commands.spawn((crate::menu::MatchEntity, row, Drawn)).with_children(|r| {
            // On a dark plate like the match bar's, so it reads over bright roofs.
            r.spawn((
                Node { padding: UiRect::axes(px(10), px(2)), border: UiRect::bottom(px(2)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7 * alpha)),
                BorderColor::all(Color::srgba(1.0, 0.15, 0.1, alpha)),
            ))
            .with_children(|l| {
                l.spawn(crate::hud::text(kind.text(), 17.0, Color::srgba(1.0, 0.32, 0.22, alpha)));
            });
        });
    }
    let Ok((map_node, computed, node)) = minimap.single() else { return };
    let side = crate::hud::minimap_side(computed, node);
    let (w, h) = (sim.world.map.width.to_f32(), sim.world.map.height.to_f32());
    let pings = alerts.pings.clone();
    commands.entity(map_node).with_children(|m| {
        for (at, age) in pings {
            let r = ping_radius(age);
            let (x, y) = (at.x.to_f32() / w * side, at.y.to_f32() / h * side);
            m.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(x - r),
                    top: px(y - r),
                    width: px(2.0 * r),
                    height: px(2.0 * r),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(Color::srgb(1.0, 0.15, 0.1)),
                ZIndex(10),
                Drawn,
            ));
        }
    });
}

/// A ping's radius in minimap pixels `age` seconds in: closing in from 30 to 6, then pulsing
/// between 6 and 9 twice a second.
fn ping_radius(age: f32) -> f32 {
    if age < PING_SHRINK_SECS {
        30.0 - 24.0 * age / PING_SHRINK_SECS
    } else {
        6.0 + 3.0 * (0.5 - 0.5 * (std::f32::consts::TAU * 2.0 * (age - PING_SHRINK_SECS)).cos())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use g2_rules::ids::*;
    use g2_sim::{CommandKind, Vec2};

    /// A GLA rabble squad attack-moving into the EU base: the first hit on a building
    /// notifies at once, no second base alert comes within 30 s however many hits land, and
    /// the next one comes once 30 s have passed; V then centres the camera there. Hits on our
    /// own stuff by ourselves never count.
    #[test]
    fn base_alerts_are_throttled_and_jumpable() {
        let mut players = crate::sim::Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], false);
        players[1].ai = None;
        players[1].starting_credits = 100_000;
        let mut sim = crate::sim::Sim::new(g2_rules::crater_ridge(), None, 1, players);
        let (me, them) = (sim.local, PlayerId(1));
        let w = &mut sim.world;
        let worker = w.entities().find(|e| e.owner == Some(them) && e.def == GLA_WORKER).unwrap().id;
        w.submit(them, CommandKind::Build { builder: worker, def: GLA_BARRACKS, pos: Vec2::from_int(290, 270) });
        let barracks = loop {
            w.step();
            if let Some(b) = w.entities().find(|e| e.def == GLA_BARRACKS && e.is_complete()) {
                break b.id;
            }
        };
        for _ in 0..5 {
            w.submit(them, CommandKind::Train { producer: barracks, def: GLA_RABBLE });
        }
        while w.entities().filter(|e| e.def == GLA_RABBLE).count() < 5 {
            w.step();
        }
        let squads = w.entities().filter(|e| e.def == GLA_RABBLE).map(|e| e.id).collect();
        let cc = w.entities().find(|e| e.owner == Some(me) && e.def == EU_CC).unwrap().pos;
        w.submit(them, CommandKind::AttackMove { units: squads, target: cc });

        let mut alerts = Alerts::default();
        let mut base = Vec::new();
        let mut hits_between = 0;
        for _ in 0..30 * 150 {
            sim.world.step();
            alerts.observe(&sim.world, me);
            let on_base = sim.world.events().iter().any(|e| {
                matches!(e, Event::Hit { victim: Some(v), target_def, .. }
                    if *v == me && sim.world.rules.def(*target_def).is_building())
            });
            for n in alerts.fresh.drain(..) {
                assert_ne!(n.kind, Kind::Collector, "no collector of ours is out yet");
                if n.kind == Kind::Base {
                    base.push(n);
                }
            }
            if base.len() == 1 && on_base {
                hits_between += 1;
            }
            if base.len() == 2 {
                break;
            }
        }
        assert_eq!(base.len(), 2, "two base alerts in 150 s of attack");
        assert!(hits_between > 3, "the base was hit {hits_between} times between them");
        assert!(base[1].tick - base[0].tick >= Kind::Base.interval(), "{:?}", base);

        // V centres on the latest alert.
        let mut app = App::new();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyV);
        app.insert_resource(keys).insert_resource(sim).insert_resource(Alerts { last: Some(base[1].at), ..default() });
        app.world_mut().spawn(RtsCamera { pivot: Vec3::ZERO, distance: 100.0, yaw: 0.0, pitch: 1.0 });
        app.add_systems(Update, jump);
        app.update();
        let pivot = app.world_mut().query::<&RtsCamera>().single(app.world()).unwrap().pivot;
        let want = bevy::math::Vec2::new(base[1].at.x.to_f32(), base[1].at.y.to_f32());
        assert!((bevy::math::Vec2::new(pivot.x, pivot.z) - want).length() < 0.01, "{pivot} vs {want}");
    }

    #[test]
    fn pings_close_in_then_pulse() {
        assert_eq!(ping_radius(0.0), 30.0);
        assert!((ping_radius(PING_SHRINK_SECS) - 6.0).abs() < 1e-4);
        let pulse: Vec<f32> = (0..20).map(|k| ping_radius(PING_SHRINK_SECS + k as f32 * 0.05)).collect();
        assert!(pulse.iter().all(|&r| (6.0..=9.0).contains(&r)) && pulse.iter().any(|&r| r > 8.5));
    }
}
