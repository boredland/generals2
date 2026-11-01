//! The rules files (`crates/g2-rules/data`, `docs/RULES.md`): errors name the file, line and
//! field; `G2_RULES_DIR` replaces a file; a changed value changes the network build id.

use g2_rules::data::{FILES, LoadError, load, parse};
use g2_sim::net::build_id;

/// The embedded files with `name`'s text edited by `edit`.
fn with(name: &str, edit: impl FnOnce(&str) -> String) -> [(&'static str, &'static str); 8] {
    let mut files = FILES;
    let file = files.iter_mut().find(|(n, _)| *n == name).expect(name);
    file.1 = Box::leak(edit(file.1).into_boxed_str());
    files
}

fn line_of(text: &str, needle: &str) -> usize {
    text[..text.find(needle).expect(needle)].matches('\n').count() + 1
}

fn units() -> &'static str {
    FILES.iter().find(|(n, _)| *n == "units.toml").unwrap().1
}

fn err(files: [(&'static str, &'static str); 8]) -> LoadError {
    match parse(files) {
        Ok(_) => panic!("bad rules accepted"),
        Err(e) => e,
    }
}

#[test]
fn the_embedded_files_load_as_the_game_does() {
    let parsed = parse(FILES).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(build_id(&parsed), build_id(g2_rules::rules()));
}

/// Loading every file once takes about a millisecond, so startup doesn't notice it.
#[test]
fn parsing_is_quick() {
    let start = std::time::Instant::now();
    let runs = 20;
    for _ in 0..runs {
        parse(FILES).unwrap();
    }
    let each = start.elapsed() / runs;
    println!("rules parse: {each:?} each");
    assert!(each < std::time::Duration::from_millis(100), "{each:?}");
}

#[test]
fn a_wrong_type_names_its_file_line_and_field() {
    let e = err(with("units.toml", |t| t.replacen("hp = 200\n", "hp = \"lots\"\n", 1)));
    assert_eq!(e.file, "units.toml");
    assert_eq!(e.field, "eu_recon.hp");
    assert_eq!(e.line, line_of(units(), "hp = 200\n"));
    assert_eq!(e.to_string(), format!("units.toml:{}: eu_recon.hp: expected a whole number", e.line));
}

#[test]
fn bad_input_is_refused_with_where_it_is() {
    // Floats that could differ by machine never reach the sim.
    let e = err(with("units.toml", |t| t.replacen("speed = 14\n", "speed = 1e1\n", 1)));
    assert_eq!((e.field.as_str(), e.message.contains("3.5")), ("eu_dozer.speed", true), "{e}");
    // A misspelt field isn't skipped.
    let e = err(with("units.toml", |t| t.replacen("hp = 250\n", "hitpoints = 250\n", 1)));
    assert_eq!(e.field, "eu_dozer.hp", "{e}");
    let e = err(with("units.toml", |t| t.replacen("hp = 250\n", "hp = 250\nhitpoint = 1\n", 1)));
    assert_eq!((e.field.as_str(), e.message.as_str()), ("eu_dozer.hitpoint", "unknown field"), "{e}");
    // A reference to nothing.
    let e = err(with("units.toml", |t| t.replacen("\"eu_supply\", \"eu_power\"", "\"eu_supply\", \"eu_powr\"", 1)));
    assert_eq!(e.field, "eu_dozer.builds[1]", "{e}");
    assert!(e.message.contains("eu_powr"), "{e}");
    // An id the code uses, gone.
    let e = err(with("units.toml", |t| t.replacen("id = \"eu_dozer\"", "id = \"eu_bulldozer\"", 1)));
    assert_eq!((e.field.as_str(), e.line), ("eu_dozer", 0), "{e}");
    // Not TOML at all.
    let e = err(with("weapons.toml", |t| format!("{t}\n[rifles\n")));
    assert_eq!(e.file, "weapons.toml");
    assert!(e.line > 1, "{e}");
}

#[test]
fn a_mod_folder_replaces_only_its_files_and_changes_the_build_id() {
    let dir = std::env::temp_dir().join(format!("g2-rules-mod-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let base = load(None).unwrap();
    assert_eq!(build_id(&base), build_id(g2_rules::rules()));
    // An empty folder: the built-in rules.
    assert_eq!(build_id(&load(Some(&dir)).unwrap()), build_id(&base));

    std::fs::write(dir.join("units.toml"), units().replacen("hp = 200\n", "hp = 201\n", 1)).unwrap();
    let modded = load(Some(&dir)).unwrap();
    let recon = modded.def(g2_rules::ids::EU_RECON);
    assert_eq!((recon.name, recon.hp), ("EU Recon Squad", 201));
    assert_eq!(modded.defs.len(), base.defs.len());
    assert_ne!(build_id(&modded), build_id(&base), "a modded value must refuse unmodded machines");

    std::fs::write(dir.join("units.toml"), units().replacen("hp = 200\n", "hp = 2x\n", 1)).unwrap();
    let Err(e) = load(Some(&dir)) else { panic!("bad rules accepted") };
    assert!(e.file.ends_with("units.toml") && e.file.contains("g2-rules-mod-"), "{e}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_mod_may_add_a_unit() {
    let files = with("units.toml", |t| {
        format!(
            "{t}\n[[unit]]\nid = \"eu_scout\"\nname = \"EU Scout\"\nfaction = \"eu\"\nhp = 90\narmour = \"light\"\n\
             cost = 200\nbuild_time = 7.5\nradius = 2\nspeed = 20.5\nweapon = \"rifles\"\n"
        )
    });
    let mut files = files;
    let barracks = files.iter_mut().find(|(n, _)| *n == "buildings.toml").unwrap();
    let produces = "produces = [\"eu_recon\", \"eu_rockets\"]";
    barracks.1 =
        Box::leak(barracks.1.replacen(produces, &produces.replace("]", ", \"eu_scout\"]"), 1).into_boxed_str());
    let r = parse(files).unwrap_or_else(|e| panic!("{e}"));
    let base = g2_rules::rules();
    assert_eq!(r.defs.len(), base.defs.len() + 1);
    let scout = r.find("EU Scout").unwrap();
    assert_eq!(scout.0 as usize, base.defs.len(), "after every id the code knows");
    let d = r.def(scout);
    assert_eq!((d.build_ticks, d.speed()), (225, g2_sim::Fx::from_ratio(205, 300)));
    assert!(r.def(g2_rules::ids::EU_BARRACKS).produces.contains(&scout));
}
