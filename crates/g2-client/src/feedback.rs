//! Where an order went, as OpenRA's `OrderEffects` shows it: a ring that closes in on the
//! ground at the target (green for a move, orange for an attack-move, teal for a rally point),
//! and a target unit or building's ring flashing (red for an attack, white for gathering or
//! getting in, OpenRA's white overlay). Only the local player's own orders, from `input`;
//! client-side, gone within `MARK_SECS`.

use bevy::prelude::*;
use g2_sim::{CommandKind, EntityId};

use crate::sim::Sim;

/// About as long as OpenRA's `moveflsh` plays (its frames at 80 ms each).
pub const MARK_SECS: f32 = 0.5;
/// A target flashes twice, each flash on for 2 ticks and off for 2 at OpenRA's 40 ms a tick
/// (`ActorFlashCount: 2`, `ActorFlashInterval: 2`).
const FLASH_ON: f32 = 0.08;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Move,
    AttackMove,
    Attack,
    /// Gather or garrison: a unit or building of someone's as the target, not an enemy.
    Use,
    Rally,
}

#[derive(Clone, Copy, Debug)]
pub struct Mark {
    pub kind: Kind,
    pub at: g2_sim::Vec2,
    pub target: Option<EntityId>,
    pub age: f32,
}

/// The local player's recent orders, newest last.
#[derive(Resource, Default)]
pub struct Marks(pub Vec<Mark>);

impl Marks {
    /// Records the mark of an order `input` gives at `at`, if it has one.
    pub fn order(&mut self, kind: &CommandKind, at: g2_sim::Vec2) {
        let (kind, target) = match *kind {
            CommandKind::Move { .. } => (Kind::Move, None),
            CommandKind::AttackMove { .. } => (Kind::AttackMove, None),
            CommandKind::Attack { target, .. } => (Kind::Attack, Some(target)),
            CommandKind::Gather { pile, .. } => (Kind::Use, Some(pile)),
            CommandKind::Garrison { container, .. } => (Kind::Use, Some(container)),
            CommandKind::Construct { site, .. } => (Kind::Use, Some(site)),
            CommandKind::SetRally { .. } => (Kind::Rally, None),
            _ => return,
        };
        // One order often splits into two commands (fighters attack, the rest walk there):
        // one mark for the click, the stronger one.
        if let Some(last) = self.0.last_mut().filter(|m| m.age == 0.0 && m.at == at) {
            if kind == Kind::Attack || last.kind == Kind::Move {
                *last = Mark { kind, at, target, age: 0.0 };
            }
            return;
        }
        self.0.push(Mark { kind, at, target, age: 0.0 });
    }
}

pub struct FeedbackPlugin;

impl Plugin for FeedbackPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Marks>().add_systems(Update, draw.after(crate::input::InputSet));
    }
}

pub fn color(kind: Kind) -> Color {
    match kind {
        Kind::Move => Color::srgb(0.3, 1.0, 0.4),
        Kind::AttackMove => Color::srgb(1.0, 0.6, 0.15),
        Kind::Attack => Color::srgb(1.0, 0.2, 0.15),
        Kind::Use => Color::WHITE,
        Kind::Rally => crate::hud::TEAL,
    }
}

/// Ages the marks and draws them: a ground ring that shrinks to its spot and fades, and the
/// target's flashing ring for an order on a unit or building.
fn draw(mut gizmos: Gizmos, sim: Res<Sim>, time: Res<Time>, mut marks: ResMut<Marks>) {
    let dt = time.delta_secs();
    let flat = |p: Vec3| Isometry3d::new(p + Vec3::Y * 0.5, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
    for m in &mut marks.0 {
        let t = (m.age / MARK_SECS).clamp(0.0, 1.0);
        let c = color(m.kind);
        match m.target.and_then(|id| sim.world.entity(id)) {
            Some(e) => {
                if flash_on(m.age) {
                    let r = sim.world.rules.def(e.def).radius.to_f32() + 1.5;
                    gizmos.circle(flat(sim.to_world(e.pos)), r, c);
                    gizmos.circle(flat(sim.to_world(e.pos)), r + 0.4, c);
                }
            }
            None => {
                let at = sim.to_world(m.at);
                let fade = c.with_alpha(1.0 - t);
                gizmos.circle(flat(at), 4.0 * (1.0 - t) + 1.0, fade);
                gizmos.circle(flat(at), 0.6, fade);
            }
        }
        m.age += dt;
    }
    marks.0.retain(|m| m.age < MARK_SECS);
}

/// Whether a target ring is lit `age` seconds after its order: two flashes, then dark.
fn flash_on(age: f32) -> bool {
    age < 4.0 * FLASH_ON && ((age / FLASH_ON) as u32).is_multiple_of(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A split order (squads attack, a dozer walks there) leaves one red mark on the target;
    /// orders without a place (train, deploy) leave none.
    #[test]
    fn one_mark_per_click() {
        let mut marks = Marks::default();
        let (u, t) = (EntityId { index: 1, generation: 0 }, EntityId { index: 2, generation: 0 });
        let at = g2_sim::Vec2::from_int(10, 10);
        marks.order(&CommandKind::Attack { units: vec![u], target: t }, at);
        marks.order(&CommandKind::Move { units: vec![u], target: at }, at);
        marks.order(&CommandKind::Deploy { units: vec![u], deployed: true }, at);
        assert_eq!(marks.0.len(), 1);
        assert_eq!((marks.0[0].kind, marks.0[0].target), (Kind::Attack, Some(t)));
        assert!(flash_on(0.0) && !flash_on(0.1) && flash_on(0.17) && !flash_on(0.4));
    }
}
