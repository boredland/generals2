//! Bridge between the deterministic `g2_sim::World` and Bevy. The sim owns all game state;
//! Bevy only mirrors it for rendering and turns input into `CommandKind`s.

use bevy::prelude::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::{CommandKind, Faction, MatchSetup, PlayerId, PlayerSetup, TICKS_PER_SECOND, World};

/// The local human player.
pub const LOCAL: PlayerId = PlayerId(0);

#[derive(Resource)]
pub struct Sim {
    pub world: World,
    /// Sim ticks per fixed step: 0 = paused, 1 = normal, up to 8× fast-forward.
    pub speed: u32,
}

impl Sim {
    pub fn new(seed: u64, factions: [Faction; 2], spectate: bool) -> Sim {
        let setup = MatchSetup {
            map: crater_ridge(),
            seed,
            players: vec![
                PlayerSetup { faction: factions[0], starting_credits: 5_000, ai: spectate },
                PlayerSetup { faction: factions[1], starting_credits: 5_000, ai: true },
            ],
        };
        Sim { world: World::new(&setup, rules()), speed: 1 }
    }

    pub fn order(&mut self, kind: CommandKind) {
        if !self.world.is_ai(LOCAL) {
            self.world.submit(LOCAL, kind);
        }
    }
}

/// Fixed-point sim position → Bevy world (sim x → +X, sim y → +Z, ground at Y = 0).
pub fn to_world(p: g2_sim::Vec2) -> Vec3 {
    Vec3::new(p.x.to_f32(), 0.0, p.y.to_f32())
}

/// Bevy ground point → sim position, rounded to the sim's fixed-point grid.
pub fn to_sim(p: Vec3) -> g2_sim::Vec2 {
    let fx = |v: f32| g2_sim::Fx((v * (1u32 << g2_sim::Fx::FRAC_BITS) as f32).round() as i64);
    g2_sim::Vec2::new(fx(p.x), fx(p.z))
}

/// Sim positions at the start of the current tick, for interpolating between ticks.
#[derive(Resource, Default)]
pub struct Previous(pub std::collections::BTreeMap<g2_sim::EntityId, g2_sim::Vec2>);

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(TICKS_PER_SECOND as f64))
            .init_resource::<Previous>()
            .add_systems(FixedUpdate, step_sim);
    }
}

fn step_sim(mut sim: ResMut<Sim>, mut prev: ResMut<Previous>) {
    for _ in 0..sim.speed {
        prev.0 = sim.world.entities().map(|e| (e.id, e.pos)).collect();
        sim.world.step();
        if sim.world.winner().is_some() {
            sim.speed = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_conversion_round_trips_on_the_fixed_grid() {
        let p = g2_sim::Vec2::new(g2_sim::Fx::from_ratio(1234, 10), g2_sim::Fx::from_int(77));
        assert_eq!(to_sim(to_world(p)), p);
    }

    /// Runs the real Bevy schedule (no renderer) for `secs` of virtual time.
    fn ticks_after(speed: u32, secs: u32) -> u32 {
        use bevy::time::TimeUpdateStrategy;
        let frame = std::time::Duration::from_millis(10);
        let mut app = App::new();
        let mut sim = Sim::new(1, [Faction::Eu, Faction::Gla], true);
        sim.speed = speed;
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(frame))
            .insert_resource(sim)
            .add_plugins(SimPlugin);
        for _ in 0..secs * 100 {
            app.update();
        }
        app.world().resource::<Sim>().world.tick()
    }

    #[test]
    fn fixed_update_drives_the_sim_at_30_hz_and_scales_with_speed() {
        let normal = ticks_after(1, 4);
        assert!((118..=120).contains(&normal), "{normal} ticks in 4 s");
        let fast = ticks_after(8, 4);
        assert!((8 * 118..=8 * 120).contains(&fast), "{fast} ticks in 4 s at x8");
        assert_eq!(ticks_after(0, 2), 0, "paused");
    }
}
