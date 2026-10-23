//! Headless tooling for the simulation.
//!
//!   g2-tools verify <file.g2rp>                       re-simulate a replay, report the first divergence
//!   g2-tools record <out.g2rp> [--ticks N] [--seed S] [--factions eu,gla] [--map NAME]   AI match → replay
//!   g2-tools bench [--ticks N] [--map NAME]           ticks per second for an AI-vs-AI match
//!   g2-tools summary <file.g2rp>                      print the final economy of a replay
//!   g2-tools maps                                     list importable Generals / Zero Hour maps
//!
//! `--map` takes "Crater Ridge" (built in, the default) or `<gen|zh>:<name>` from `maps`.

use std::process::ExitCode;
use std::time::Instant;

use g2_maps::resolve;
use g2_rules::rules;
use g2_sim::{Faction, MatchSetup, PlayerId, PlayerSetup, Replay, TICKS_PER_SECOND, World, record, verify_replay};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("verify") => verify(&args[1..]),
        Some("record") => record_cmd(&args[1..]),
        Some("bench") => bench(&args[1..]),
        Some("summary") => summary(&args[1..]),
        Some("maps") => maps(),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: g2-tools <verify FILE | record OUT [--ticks N] [--seed S] [--factions A,B] [--map NAME] | bench [--ticks N] [--map NAME] | summary FILE | maps>";

fn flag<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> Result<T, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(default),
        Some(i) => args.get(i + 1).and_then(|v| v.parse().ok()).ok_or_else(|| format!("{name} needs a valid value")),
    }
}

fn faction(s: &str) -> Result<Faction, String> {
    match s.to_ascii_lowercase().as_str() {
        "eu" => Ok(Faction::Eu),
        "apa" => Ok(Faction::Apa),
        "gla" => Ok(Faction::Gla),
        _ => Err(format!("unknown faction {s:?} (eu, apa, gla)")),
    }
}

fn ai_setup(args: &[String]) -> Result<MatchSetup, String> {
    let factions: String = flag(args, "--factions", "eu,gla".to_owned())?;
    let players = factions
        .split(',')
        .map(|f| Ok(PlayerSetup { faction: faction(f)?, starting_credits: 5_000, ai: true }))
        .collect::<Result<Vec<_>, String>>()?;
    let map = resolve(&flag(args, "--map", "Crater Ridge".to_owned())?)?;
    if map.spawns.len() < players.len() {
        return Err(format!("{} has {} player starts, need {}", map.name, map.spawns.len(), players.len()));
    }
    Ok(MatchSetup { map, seed: flag(args, "--seed", 1u64)?, players })
}

fn maps() -> Result<(), String> {
    let root = g2_maps::vendored_root();
    let entries = g2_maps::catalogue(&root);
    if entries.is_empty() {
        return Err(format!("no maps under {}: git submodule update --init", root.display()));
    }
    println!("Crater Ridge  (built in, 2 starts)");
    for e in entries {
        let map = std::fs::read(&e.path)
            .map_err(|err| err.to_string())
            .and_then(|b| g2_maps::load(&b).map(|m| m.to_map_def(&e.name)).map_err(|err| err.to_string()));
        match map {
            Ok(m) if m.spawns.len() >= 2 => println!(
                "{}  ({} starts, {:.0}x{:.0}, {} supplies, {} oil)",
                e.name,
                m.spawns.len(),
                m.width.to_f32(),
                m.height.to_f32(),
                m.supply_piles.len(),
                m.oil_wells.len()
            ),
            Ok(_) => {} // campaign and test maps: no skirmish starts
            Err(err) => println!("{}  (unreadable: {err})", e.name),
        }
    }
    Ok(())
}

fn load(path: Option<&String>) -> Result<Replay, String> {
    let path = path.ok_or(USAGE)?;
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    Replay::decode(&bytes).map_err(|e| format!("{path}: {e:?}"))
}

fn verify(args: &[String]) -> Result<(), String> {
    let replay = load(args.first())?;
    let map = resolve(&replay.map)?;
    let start = Instant::now();
    match verify_replay(&replay, map, rules()) {
        Ok(hash) => {
            println!(
                "ok: {} ticks, {} checkpoints, {} commands, final hash {hash:016x} ({:.2?})",
                replay.length_ticks,
                replay.hashes.len(),
                replay.commands.len(),
                start.elapsed()
            );
            Ok(())
        }
        Err(d) => Err(format!("DIVERGED at tick {}: expected {:016x}, got {:016x}", d.tick, d.expected, d.actual)),
    }
}

fn record_cmd(args: &[String]) -> Result<(), String> {
    let out = args.first().filter(|a| !a.starts_with("--")).ok_or(USAGE)?;
    let setup = ai_setup(&args[1..])?;
    let ticks = flag(&args[1..], "--ticks", 30 * 600u32)?;
    let (world, replay) = record(World::new(&setup, rules()), &setup, ticks, |_| {});
    std::fs::write(out, replay.encode()).map_err(|e| format!("{out}: {e}"))?;
    println!("wrote {out}: {ticks} ticks, final hash {:016x}", world.state_hash());
    print_economy(&world);
    Ok(())
}

fn bench(args: &[String]) -> Result<(), String> {
    let setup = ai_setup(args)?;
    let ticks = flag(args, "--ticks", 30 * 1800u32)?;
    let mut world = World::new(&setup, rules());
    let start = Instant::now();
    for _ in 0..ticks {
        world.step();
    }
    let secs = start.elapsed().as_secs_f64();
    println!(
        "{ticks} ticks ({} min game time) in {secs:.3} s: {:.0} ticks/s, {:.0}x real time, {} entities at end",
        ticks / TICKS_PER_SECOND / 60,
        ticks as f64 / secs,
        ticks as f64 / secs / TICKS_PER_SECOND as f64,
        world.entities().count()
    );
    Ok(())
}

fn summary(args: &[String]) -> Result<(), String> {
    let replay = load(args.first())?;
    let map = resolve(&replay.map)?;
    let setup = MatchSetup { map, seed: replay.seed, players: replay.players.clone() };
    let mut world = World::new(&setup, rules());
    for c in &replay.commands {
        world.schedule(c.clone());
    }
    while world.tick() < replay.length_ticks {
        world.step();
    }
    print_economy(&world);
    Ok(())
}

fn print_economy(world: &World) {
    for (i, p) in world.players().iter().enumerate() {
        let me = Some(PlayerId(i as u8));
        let count = |pred: &dyn Fn(&g2_sim::Def) -> bool| {
            world.entities().filter(|e| e.owner == me && pred(world.rules.def(e.def))).count()
        };
        println!(
            "  P{i} {:<22} credits {:>6}  oil {:>5}  power {:>3}/{:<3}  buildings {:>2}  collectors {:>2}{}",
            p.faction.name(),
            p.credits,
            p.oil,
            p.power_drain,
            p.power_supply,
            count(&|d| d.is_building()),
            count(&|d| d.collector().is_some()),
            if p.defeated { "  DEFEATED" } else { "" }
        );
    }
}
