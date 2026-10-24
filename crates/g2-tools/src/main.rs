//! Headless tooling for the simulation.
//!
//!   g2-tools verify <file.g2rp>                       re-simulate a replay, report the first divergence
//!   g2-tools record <out.g2rp> [--ticks N] [--seed S] [--factions eu,gla] [--ai L] [--map NAME]   AI match → replay
//!   g2-tools bench [--ticks N] [--ai L] [--map NAME]  ticks per second for an AI-vs-AI match
//!   g2-tools survey A B [--seeds N] [--map NAME]      AI level A against B: every faction pairing, both starts (B may be onslaught)
//!   g2-tools artillery LEVEL [--map NAME]             where LEVEL's artillery stands in its fights, against Easy
//!   g2-tools summary <file.g2rp>                      print the final economy of a replay
//!   g2-tools maps                                     list importable Generals / Zero Hour maps
//!   g2-tools alpha DIR [--textures OUT]                index a 2013 alpha install you own; optionally export its textures as PNG
//!
//! `--map` takes "Crater Ridge" (built in, the default) or `<gen|zh>:<name>` from `maps`.
//! `--factions` entries may name a general: `eu:bulldog,gla:junkyard`. AI levels: easy,
//! normal (`--ai`, default easy, applies to every player).

use std::process::ExitCode;
use std::time::Instant;

use g2_maps::resolve;
use g2_rules::rules;
use g2_sim::{
    AiLevel, Faction, MatchSetup, PlayerId, PlayerSetup, Replay, TICKS_PER_SECOND, World, record, verify_replay,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("verify") => verify(&args[1..]),
        Some("record") => record_cmd(&args[1..]),
        Some("bench") => bench(&args[1..]),
        Some("survey") => survey(&args[1..]),
        Some("summary") => summary(&args[1..]),
        Some("artillery") => artillery(&args[1..]),
        Some("maps") => maps(),
        Some("alpha") => alpha(&args[1..]),
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

const USAGE: &str = "usage: g2-tools <verify FILE | record OUT [--ticks N] [--seed S] [--factions A,B] [--ai L] [--map NAME] | bench [--ticks N] [--ai L] [--map NAME] | survey A B [--seeds N] [--map NAME] | summary FILE | maps | alpha DIR [--textures OUT]>";

fn level(s: &str) -> Result<AiLevel, String> {
    match s.to_ascii_lowercase().as_str() {
        "easy" => Ok(AiLevel::Easy),
        "normal" => Ok(AiLevel::Normal),
        "onslaught" => Ok(AiLevel::Onslaught),
        _ => Err(format!("unknown AI level {s:?} (easy, normal, onslaught)")),
    }
}

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
    let ai = level(&flag(args, "--ai", "easy".to_owned())?)?;
    let players = factions
        .split(',')
        .map(|entry| {
            let (f, g) = entry.split_once(':').map_or((entry, None), |(f, g)| (f, Some(g)));
            let faction = faction(f)?;
            let general = match g {
                None => None,
                Some(name) => {
                    let id = rules().find_general(name).ok_or_else(|| format!("unknown general {name:?}"))?;
                    if rules().general(id).faction != faction {
                        return Err(format!("{} is not a {f} general", rules().general(id).name));
                    }
                    Some(id)
                }
            };
            Ok(PlayerSetup { faction, starting_credits: 5_000, ai: Some(ai), general })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let map = resolve(&flag(args, "--map", "Crater Ridge".to_owned())?)?;
    if map.spawns.len() < players.len() {
        return Err(format!("{} has {} player starts, need {}", map.name, map.spawns.len(), players.len()));
    }
    Ok(MatchSetup { map, seed: flag(args, "--seed", 1u64)?, players })
}

/// Level `a` against level `b` on one map: all nine faction pairings, each from both starts,
/// for `--seeds` seeds, 20 minutes at most. Prints wins, undecided matches and lengths.
fn survey(args: &[String]) -> Result<(), String> {
    let (a, b) = match args {
        [a, b, ..] => (level(a)?, level(b)?),
        _ => return Err(USAGE.to_owned()),
    };
    if a == AiLevel::Onslaught {
        return Err("the onslaught goes second: survey LEVEL onslaught".into());
    }
    // The onslaught's waves don't depend on its seat, so its matches aren't swapped; they run
    // until well after its last wave instead of the usual 20 minutes.
    let onslaught = b == AiLevel::Onslaught;
    let cap = if onslaught {
        g2_sim::onslaught::wave_tick(g2_sim::onslaught::WAVES) + 10 * 60 * TICKS_PER_SECOND
    } else {
        20 * 60 * TICKS_PER_SECOND
    };
    let swaps: &[bool] = if onslaught { &[false] } else { &[false, true] };
    let map = resolve(&flag(args, "--map", "Crater Ridge".to_owned())?)?;
    let seeds: u64 = flag(args, "--seeds", 1u64)?;
    let (mut wins, mut undecided, mut lengths) = ([0u32; 2], Vec::new(), Vec::new());
    for seed in 1..=seeds {
        for fa in Faction::ALL {
            for fb in Faction::ALL {
                for &swap in swaps {
                    let slots = if swap { [(fb, b), (fa, a)] } else { [(fa, a), (fb, b)] };
                    let players = slots
                        .map(|(faction, l)| PlayerSetup {
                            faction,
                            starting_credits: 5_000,
                            ai: Some(l),
                            general: None,
                        })
                        .to_vec();
                    let setup = MatchSetup { map: map.clone(), seed, players };
                    let mut w = World::new(&setup, rules());
                    while w.winner().is_none() && w.tick() < cap {
                        w.step();
                    }
                    match w.winner() {
                        // Slot 0 is `a` unless swapped.
                        Some(p) => {
                            wins[usize::from((p.0 == 0) == swap)] += 1;
                            lengths.push(w.tick() / TICKS_PER_SECOND);
                        }
                        None => undecided
                            .push(format!("{:?}/{:?} v {:?}/{:?}", slots[0].0, slots[0].1, slots[1].0, slots[1].1)),
                    }
                }
            }
        }
    }
    lengths.sort();
    let median = lengths.get(lengths.len() / 2).copied().unwrap_or(0);
    println!(
        "{} vs {}: {}-{} ({} undecided{}), length {}..{} s, median {median} s",
        a.name(),
        b.name(),
        wins[0],
        wins[1],
        undecided.len(),
        if undecided.is_empty() { String::new() } else { format!(": {}", undecided.join(", ")) },
        lengths.first().unwrap_or(&0),
        lengths.last().unwrap_or(&0)
    );
    Ok(())
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
                "{}  ({} starts, {:.0}x{:.0}, {} supplies, {} oil){}",
                e.name,
                m.spawns.len(),
                m.width.to_f32(),
                m.height.to_f32(),
                m.supply_piles.len(),
                m.oil_wells.len(),
                if g2_maps::BRIDGE_ONLY.contains(&e.name.as_str()) { "  starts joined by bridges" } else { "" }
            ),
            Ok(_) => {} // campaign and test maps: no skirmish starts
            Err(err) => println!("{}  (unreadable: {err})", e.name),
        }
    }
    Ok(())
}

/// Indexes an alpha install and reports what it holds; `--textures OUT` writes every flat
/// texture's largest level as `OUT/<asset path>.png`. The output stays on this machine: it
/// is EA's art, read from the user's own copy.
fn alpha(args: &[String]) -> Result<(), String> {
    use g2_frostbite::{Alpha, res_type, texture};
    let dir = args.first().filter(|a| !a.starts_with("--")).ok_or(USAGE)?;
    let t0 = Instant::now();
    let a = Alpha::open(std::path::Path::new(dir)).map_err(|e| e.to_string())?;
    println!(
        "{} superbundles, {} bundles: {} ebx assets, {} resources, {} chunks ({:.1} s)",
        a.superbundles,
        a.bundles,
        a.ebx.len(),
        a.res.len(),
        a.chunk_count(),
        t0.elapsed().as_secs_f32()
    );
    let mut kinds = std::collections::BTreeMap::<u32, usize>::new();
    for e in a.res.values() {
        *kinds.entry(e.res_type.unwrap_or_default()).or_default() += 1;
    }
    let label = |t: u32| match t {
        res_type::TEXTURE => "texture",
        res_type::MESH_SET => "mesh set",
        _ => "",
    };
    for (t, n) in &kinds {
        println!("  res {t:08x} {n:6} {}", label(*t));
    }
    let Some(out) = args.iter().position(|a| a == "--textures").map(|i| args.get(i + 1).ok_or(USAGE)) else {
        return Ok(());
    };
    let out = std::path::Path::new(out?);
    let (mut written, mut skipped) = (0, std::collections::BTreeMap::<String, usize>::new());
    for (name, e) in a.res_of(res_type::TEXTURE) {
        let img = match texture::decode(&a, e) {
            Ok(img) => img,
            Err(err) => {
                *skipped.entry(err.to_string()).or_default() += 1;
                continue;
            }
        };
        let path = out.join(format!("{name}.png"));
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().and_then(|mut w| w.write_image_data(&img.rgba)).map_err(|e| e.to_string())?;
        written += 1;
    }
    println!("{written} textures written to {}", out.display());
    for (why, n) in skipped {
        println!("  skipped {n}: {why}");
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
    replay.check(&map, rules())?;
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

/// Where a level's artillery stands in its fights. For every faction pairing, both starts,
/// against Easy: every 2 seconds, for each of the probed player's guns out of its base (more
/// than 50 from every own building) while its line (armed non-artillery in the field) has a
/// visible enemy within 60, how far the gun stands behind the line's front: positive when the
/// gun is farther from the nearest visible enemy than the line's closest unit. Also how many
/// guns were built, how many ever left the base, and how many died.
fn artillery(args: &[String]) -> Result<(), String> {
    let probed = level(args.first().ok_or(USAGE)?)?;
    let map = resolve(&flag(args, "--map", "Crater Ridge".to_owned())?)?;
    let r = rules();
    let is_gun = |d: g2_sim::DefId| {
        let d = r.def(d);
        d.weapon_for(d.deploy.is_some()).is_some_and(|w| w.shell.is_some())
    };
    let (mut behind, mut samples, mut built, mut left, mut lost, mut wins) = (Vec::new(), 0u32, 0, 0, 0, 0);
    for fa in Faction::ALL {
        for fb in Faction::ALL {
            for seat in 0..2u8 {
                let me = PlayerId(seat);
                let mut players = vec![
                    PlayerSetup { faction: fa, starting_credits: 5_000, ai: Some(probed), general: None },
                    PlayerSetup { faction: fb, starting_credits: 5_000, ai: Some(AiLevel::Easy), general: None },
                ];
                if seat == 1 {
                    players.swap(0, 1);
                }
                let setup = MatchSetup { map: map.clone(), seed: 1, players };
                let mut w = World::new(&setup, r);
                let (mut guns, mut out) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
                while w.winner().is_none() && w.tick() < 20 * 60 * TICKS_PER_SECOND {
                    w.step();
                    for e in w.events() {
                        if let g2_sim::world::Event::Hit { killed: true, target, .. } = e
                            && guns.contains(target)
                        {
                            lost += 1;
                        }
                    }
                    if !w.tick().is_multiple_of(2 * TICKS_PER_SECOND) {
                        continue;
                    }
                    let vision = w.vision(me);
                    let enemies: Vec<_> = w
                        .entities()
                        .filter(|e| e.owner.is_some_and(|o| o != me) && vision.is_visible(e.pos))
                        .map(|e| e.pos)
                        .collect();
                    let buildings: Vec<_> = w
                        .entities()
                        .filter(|e| e.owner == Some(me) && r.def(e.def).is_building())
                        .map(|e| e.pos)
                        .collect();
                    let in_field = |p: g2_sim::Vec2| buildings.iter().all(|b| b.distance(p).to_f32() > 50.0);
                    let nearest =
                        |p: g2_sim::Vec2| enemies.iter().map(|q| q.distance(p).to_f32()).fold(f32::MAX, f32::min);
                    let mine: Vec<_> = w
                        .entities()
                        .filter(|e| {
                            e.owner == Some(me)
                                && r.def(e.def).weapon_for(e.deploy != g2_sim::entity::DeployState::Packed).is_some()
                                || e.owner == Some(me) && is_gun(e.def)
                        })
                        .collect();
                    for e in &mine {
                        if is_gun(e.def) {
                            guns.insert(e.id);
                            if in_field(e.pos) {
                                out.insert(e.id);
                            }
                        }
                    }
                    let front = mine
                        .iter()
                        .filter(|e| !is_gun(e.def) && in_field(e.pos))
                        .map(|e| nearest(e.pos))
                        .fold(f32::MAX, f32::min);
                    if front > 60.0 {
                        continue;
                    }
                    for g in mine.iter().filter(|e| is_gun(e.def) && in_field(e.pos)) {
                        samples += 1;
                        behind.push(nearest(g.pos) - front);
                    }
                }
                built += guns.len();
                left += out.len();
                wins += (w.winner() == Some(me)) as u32;
            }
        }
    }
    behind.sort_by(f32::total_cmp);
    let q = |f: f32| behind.get(((behind.len() as f32 - 1.0) * f) as usize).copied().unwrap_or(f32::NAN);
    let held = behind.iter().filter(|&&d| d > 5.0).count();
    println!(
        "{}: guns built {built}, left base {left}, lost {lost}; in fights {samples} samples, behind the front by p25/p50/p75 {:.0}/{:.0}/{:.0}, more than 5 behind {}%; wins {wins}/18",
        probed.name(),
        q(0.25),
        q(0.5),
        q(0.75),
        if samples == 0 { 0 } else { held * 100 / samples as usize }
    );
    Ok(())
}

fn summary(args: &[String]) -> Result<(), String> {
    let replay = load(args.first())?;
    let map = resolve(&replay.map)?;
    replay.check(&map, rules())?;
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
            "  P{i} {:<22} {:<11} credits {:>6}  power {:>3}/{:<3}  buildings {:>2}  collectors {:>2}{}",
            p.faction.name(),
            p.general.map_or("", |g| world.rules.general(g).name),
            p.credits,
            p.power_drain,
            p.power_supply,
            count(&|d| d.is_building()),
            count(&|d| d.collector().is_some()),
            if p.defeated { "  DEFEATED" } else { "" }
        );
    }
}
