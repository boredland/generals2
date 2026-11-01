//! The rules as data: the TOML files under `crates/g2-rules/data`, embedded in the binary and
//! parsed once at startup into the `Rules` the sim reads (`docs/RULES.md` describes them).
//! `G2_RULES_DIR` may name a folder of edited copies, read instead of the embedded ones.
//!
//! Nothing here reaches the sim as a float: a decimal such as `3.5` is read as the exact ratio
//! of two integers and converted the way the sim's own constants are (`Fx::from_ratio`, ticks
//! rounded down), so a value gives the same bits on every machine.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::path::Path;

use g2_sim::defs::{
    Ability, AbilityKind, Armour, Blast, BuildingRole, Class, CollectorSpec, Cost, Def, DefId, Deploy, Effect, Faction,
    Field, Garrison, General, GeneralUnit, MAX_ABILITIES, Modifier, Perk, PerkId, Power, PowerKind, PowerTuning, Rules,
    Shell, Special, StatusEffect, StatusKind, Targets, Upgrade, UpgradeId, Veterancy, Weapon,
};
use g2_sim::{Fx, TICKS_PER_SECOND};
use toml::Spanned;
use toml::de::{DeTable, DeValue};

/// Every rules file with its embedded text, in the order they are read.
pub const FILES: [(&str, &str); 8] = [
    ("weapons.toml", include_str!("../data/weapons.toml")),
    ("abilities.toml", include_str!("../data/abilities.toml")),
    ("buildings.toml", include_str!("../data/buildings.toml")),
    ("units.toml", include_str!("../data/units.toml")),
    ("powers.toml", include_str!("../data/powers.toml")),
    ("upgrades.toml", include_str!("../data/upgrades.toml")),
    ("perks.toml", include_str!("../data/perks.toml")),
    ("generals.toml", include_str!("../data/generals.toml")),
];

/// What is wrong with a rules file, and where: `units.toml:212: eu_recon.hp: expected a whole
/// number`. `line` is 0 for a problem with no one place (an id missing from every file).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadError {
    pub file: String,
    pub line: usize,
    pub field: String,
    pub message: String,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: ", self.file, self.line)?;
        if !self.field.is_empty() {
            write!(f, "{}: ", self.field)?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for LoadError {}

type Res<T> = Result<T, LoadError>;

/// The embedded files, or, for each one `dir` holds, its copy there.
pub fn load(dir: Option<&Path>) -> Res<Rules> {
    let mut files = FILES;
    if let Some(dir) = dir {
        for (name, text) in &mut files {
            let path = dir.join(*name);
            match std::fs::read_to_string(&path) {
                // Leaked: the rules live as long as the program and borrow their names from it.
                Ok(s) => {
                    *name = Box::leak(path.display().to_string().into_boxed_str());
                    *text = Box::leak(s.into_boxed_str());
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(LoadError {
                        file: path.display().to_string(),
                        line: 0,
                        field: String::new(),
                        message: e.to_string(),
                    });
                }
            }
        }
    }
    parse(files)
}

/// The rules from `files`, named and ordered as `FILES`.
pub fn parse(files: [(&'static str, &'static str); 8]) -> Res<Rules> {
    let files = files.map(|(name, text)| File { name, text });
    let mut docs = Vec::with_capacity(files.len());
    for f in files {
        docs.push(f.parse()?);
    }
    let [weapons, abilities, buildings, units, powers, upgrades, perks, generals] = files;
    let [d_weapons, d_abilities, d_buildings, d_units, d_powers, d_upgrades, d_perks, d_generals] = &docs[..] else {
        unreachable!("eight files")
    };

    // Ids first, so an entry may name one defined after it, in any file.
    let mut def_list = keyed(list(buildings, d_buildings, "building", &[])?)?;
    let first_unit = def_list.len();
    def_list.extend(keyed(list(units, d_units, "unit", &["ranks"])?)?);
    let def_list = ordered(def_list, crate::ids::KEYS, "buildings.toml, units.toml")?;
    let upgrade_list =
        ordered(keyed(list(upgrades, d_upgrades, "upgrade", &[])?)?, crate::upgrades::KEYS, upgrades.name)?;
    let perk_list = ordered(keyed(list(perks, d_perks, "perk", &[])?)?, crate::perks::KEYS, perks.name)?;
    let general_list =
        ordered(keyed(list(generals, d_generals, "general", &[])?)?, crate::generals::KEYS, generals.name)?;
    let mut ids = Ids {
        defs: index(&def_list, DefId),
        upgrades: index(&upgrade_list, UpgradeId),
        perks: index(&perk_list, PerkId),
        weapons: HashMap::new(),
        abilities: HashMap::new(),
    };
    for (key, v) in named(weapons, d_weapons) {
        ids.weapons.insert(key, weapon_table(v.table()?)?);
    }
    for (key, v) in named(abilities, d_abilities) {
        ids.abilities.insert(key, ability(v.table()?, &ids)?);
    }

    let ranks = match d_units.get("ranks") {
        Some(v) => {
            let mut t = Val { file: units, path: "ranks".to_owned(), v }.table()?;
            let defaults = (t.need("at", |v| v.ints::<u32, 3>())?, t.need("idle_regen", Val::int)?);
            t.done()?;
            Some(defaults)
        }
        None => None,
    };
    let defs = def_list.into_iter().map(|o| def(o.entry, &ids, o.pos >= first_unit, ranks)).collect::<Res<Vec<_>>>()?;
    let upgrades = upgrade_list.into_iter().map(|o| upgrade(o.entry, &ids)).collect::<Res<Vec<_>>>()?;
    let perks = perk_list.into_iter().map(|o| perk(o.entry, &ids)).collect::<Res<Vec<_>>>()?;
    let generals = general_list.into_iter().map(|o| general(o.entry, &ids)).collect::<Res<Vec<_>>>()?;
    let support = list(powers, d_powers, "support", &[])?
        .into_iter()
        .map(|mut e| Ok((e.need("faction", Val::faction)?, power(e, &ids)?)))
        .collect::<Res<Vec<_>>>()?;

    Ok(Rules {
        defs: leak(defs),
        generals: leak(generals),
        upgrades: leak(upgrades),
        perks: leak(perks),
        support: leak(support),
        missions: &crate::missions::MISSIONS,
    })
}

fn leak<T>(v: Vec<T>) -> &'static [T] {
    if v.is_empty() { &[] } else { Box::leak(v.into_boxed_slice()) }
}

#[derive(Clone, Copy)]
struct File {
    name: &'static str,
    text: &'static str,
}

impl File {
    fn err(&self, span: &Range<usize>, field: &str, message: impl Into<String>) -> LoadError {
        let line = self.text.get(..span.start).map_or(0, |s| s.matches('\n').count() + 1);
        LoadError { file: self.name.to_owned(), line, field: field.to_owned(), message: message.into() }
    }

    fn parse(self) -> Res<DeTable<'static>> {
        DeTable::parse(self.text)
            .map(Spanned::into_inner)
            .map_err(|e| self.err(&e.span().unwrap_or(0..0), "", e.message()))
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() { key.to_owned() } else { format!("{path}.{key}") }
}

/// One value of a file, with where it came from for errors.
struct Val<'a> {
    file: File,
    path: String,
    v: &'a Spanned<DeValue<'static>>,
}

impl<'a> Val<'a> {
    fn err(&self, message: impl Into<String>) -> LoadError {
        self.file.err(&self.v.span(), &self.path, message)
    }

    fn int<T: TryFrom<i64>>(&self) -> Res<T> {
        match self.v.get_ref() {
            DeValue::Integer(i) => i64::from_str_radix(i.as_str(), i.radix())
                .ok()
                .and_then(|n| T::try_from(n).ok())
                .ok_or_else(|| self.err(format!("{} is out of range here", i.as_str()))),
            _ => Err(self.err("expected a whole number")),
        }
    }

    fn ints<T: TryFrom<i64> + Copy + Default, const N: usize>(&self) -> Res<[T; N]> {
        let items = self.array()?;
        if items.len() != N {
            return Err(self.err(format!("expected {N} numbers")));
        }
        let mut out = [T::default(); N];
        for (o, v) in out.iter_mut().zip(&items) {
            *o = v.int()?;
        }
        Ok(out)
    }

    fn bool(&self) -> Res<bool> {
        match self.v.get_ref() {
            DeValue::Boolean(b) => Ok(*b),
            _ => Err(self.err("expected true or false")),
        }
    }

    fn str(&self) -> Res<&'static str> {
        match self.v.get_ref() {
            DeValue::String(std::borrow::Cow::Borrowed(s)) => Ok(s),
            DeValue::String(std::borrow::Cow::Owned(s)) => Ok(Box::leak(s.clone().into_boxed_str())),
            _ => Err(self.err("expected a string")),
        }
    }

    /// A whole number or a plain decimal (`3`, `3.5`), as the exact ratio `(n, d)`.
    fn ratio(&self) -> Res<(i64, i64)> {
        let bad = || self.err("expected a number such as 3 or 3.5");
        match self.v.get_ref() {
            DeValue::Integer(_) => Ok((self.int()?, 1)),
            DeValue::Float(f) => decimal(f.as_str()).ok_or_else(bad),
            _ => Err(bad()),
        }
    }

    /// World units.
    fn dist(&self) -> Res<Fx> {
        let (n, d) = self.ratio()?;
        if n.unsigned_abs() >= 1 << 40 {
            return Err(self.err("too large"));
        }
        Ok(Fx::from_ratio(n, d))
    }

    /// World units per second, stored per tick.
    fn speed(&self) -> Res<Fx> {
        let (n, d) = self.ratio()?;
        if n.unsigned_abs() >= 1 << 40 {
            return Err(self.err("too large"));
        }
        Ok(Fx::from_ratio(n, d * TICKS_PER_SECOND as i64))
    }

    /// Seconds (`2`, `6.27`, rounded down to a whole tick), or ticks as `"1 tick"`, `"4 ticks"`.
    fn time(&self) -> Res<u32> {
        if let DeValue::String(s) = self.v.get_ref() {
            return s
                .strip_suffix(" ticks")
                .or_else(|| s.strip_suffix(" tick"))
                .and_then(|n| n.parse().ok())
                .ok_or_else(|| self.err("expected seconds, or a count of ticks such as \"4 ticks\""));
        }
        let (n, d) = self.ratio()?;
        n.checked_mul(TICKS_PER_SECOND as i64)
            .and_then(|t| u32::try_from(t / d).ok())
            .ok_or_else(|| self.err("out of range"))
    }

    fn array(&self) -> Res<Vec<Val<'a>>> {
        match self.v.get_ref() {
            DeValue::Array(a) => Ok(a
                .iter()
                .enumerate()
                .map(|(i, v)| Val { file: self.file, path: format!("{}[{i}]", self.path), v })
                .collect()),
            _ => Err(self.err("expected a list")),
        }
    }

    fn table(&self) -> Res<Entry<'a>> {
        match self.v.get_ref() {
            DeValue::Table(t) => Ok(Entry::new(self.file, self.path.clone(), self.v.span(), t)),
            _ => Err(self.err("expected a table")),
        }
    }

    fn pick<T: Copy>(&self, options: &[(&str, T)]) -> Res<T> {
        let s = self.str()?;
        options.iter().find(|(k, _)| *k == s).map(|&(_, t)| t).ok_or_else(|| {
            let names: Vec<_> = options.iter().map(|(k, _)| *k).collect();
            self.err(format!("unknown {s:?}; expected one of {}", names.join(", ")))
        })
    }

    fn faction(&self) -> Res<Faction> {
        self.pick(&[("eu", Faction::Eu), ("apa", Faction::Apa), ("gla", Faction::Gla)])
    }

    fn armour(&self) -> Res<Armour> {
        self.pick(&[
            ("infantry", Armour::Infantry),
            ("light", Armour::Light),
            ("heavy", Armour::Heavy),
            ("structure", Armour::Structure),
        ])
    }

    fn status(&self) -> Res<StatusKind> {
        self.pick(&[
            ("toughness", StatusKind::Toughness),
            ("damage", StatusKind::Damage),
            ("speed", StatusKind::Speed),
            ("sight", StatusKind::Sight),
            ("stun", StatusKind::Stun),
        ])
    }

    fn lookup<T: Copy>(&self, map: &HashMap<&str, T>, what: &str) -> Res<T> {
        let s = self.str()?;
        map.get(s).copied().ok_or_else(|| self.err(format!("no {what} {s:?}")))
    }

    fn def(&self, ids: &Ids) -> Res<DefId> {
        self.lookup(&ids.defs, "unit or building")
    }

    fn defs(&self, ids: &Ids) -> Res<&'static [DefId]> {
        Ok(leak(self.array()?.iter().map(|v| v.def(ids)).collect::<Res<_>>()?))
    }
}

/// `-?digits(.digits)?` as `(n, 10^k)`; `None` for anything else (exponents, inf, nan).
fn decimal(s: &str) -> Option<(i64, i64)> {
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    if !digits(int) || !(frac.is_empty() || digits(frac)) || frac.len() > 9 {
        return None;
    }
    let den = 10_i64.pow(frac.len() as u32);
    let n = int.parse::<i64>().ok()?.checked_mul(den)?.checked_add(if frac.is_empty() {
        0
    } else {
        frac.parse().ok()?
    })?;
    Some((if neg { -n } else { n }, den))
}

/// A table's fields; each read takes one, and `done` rejects any left (a misspelt field).
struct Entry<'a> {
    file: File,
    path: String,
    span: Range<usize>,
    fields: Vec<(&'a str, &'a Spanned<DeValue<'static>>)>,
}

impl<'a> Entry<'a> {
    fn new(file: File, path: String, span: Range<usize>, t: &'a DeTable<'static>) -> Self {
        Entry { file, path, span, fields: t.iter().map(|(k, v)| (k.get_ref().as_ref(), v)).collect() }
    }

    fn take(&mut self, key: &str) -> Option<Val<'a>> {
        let i = self.fields.iter().position(|(k, _)| *k == key)?;
        let (_, v) = self.fields.remove(i);
        Some(Val { file: self.file, path: join(&self.path, key), v })
    }

    fn get<T>(&mut self, key: &str, read: impl FnOnce(&Val<'a>) -> Res<T>) -> Res<Option<T>> {
        self.take(key).map(|v| read(&v)).transpose()
    }

    fn need<T>(&mut self, key: &str, read: impl FnOnce(&Val<'a>) -> Res<T>) -> Res<T> {
        match self.take(key) {
            Some(v) => read(&v),
            None => Err(self.file.err(&self.span, &join(&self.path, key), "missing")),
        }
    }

    fn or<T>(&mut self, key: &str, default: T, read: impl FnOnce(&Val<'a>) -> Res<T>) -> Res<T> {
        Ok(self.get(key, read)?.unwrap_or(default))
    }

    fn err(&self, message: impl Into<String>) -> LoadError {
        self.file.err(&self.span, &self.path, message)
    }

    fn done(self) -> Res<()> {
        match self.fields.first() {
            Some((k, v)) => Err(self.file.err(&v.span(), &join(&self.path, k), "unknown field")),
            None => Ok(()),
        }
    }
}

/// The `[[array]]` entries of a file; any other top-level key but `extra` is refused.
fn list<'a>(file: File, doc: &'a DeTable<'static>, array: &str, extra: &[&str]) -> Res<Vec<Entry<'a>>> {
    let mut out = Vec::new();
    for (k, v) in doc.iter() {
        let key: &str = k.get_ref();
        let val = Val { file, path: key.to_owned(), v };
        if key == array {
            for (i, item) in val.array()?.iter().enumerate() {
                let mut e = item.table()?;
                e.path = format!("{array}[{i}]");
                out.push(e);
            }
        } else if !extra.contains(&key) {
            return Err(val.err(format!("unknown section; this file holds [[{array}]] entries")));
        }
    }
    Ok(out)
}

/// Each entry with its `id`, which then names it in errors.
fn keyed(list: Vec<Entry>) -> Res<Vec<(&'static str, Entry)>> {
    list.into_iter()
        .map(|mut e| {
            let id = e.need("id", Val::str)?;
            e.path = id.to_owned();
            Ok((id, e))
        })
        .collect()
}

/// The tables of a file keyed by name (`[rifles]`).
fn named<'a>(file: File, doc: &'a DeTable<'static>) -> impl Iterator<Item = (&'static str, Val<'a>)> {
    doc.iter().map(move |(k, v)| {
        let key: &'static str = match k.get_ref() {
            std::borrow::Cow::Borrowed(s) => s,
            std::borrow::Cow::Owned(s) => Box::leak(s.clone().into_boxed_str()),
        };
        (key, Val { file, path: key.to_owned(), v })
    })
}

struct Ordered<'a> {
    id: &'static str,
    /// Where it was read, across the files of its kind.
    pos: usize,
    entry: Entry<'a>,
}

/// The entries in id order: those `keys` names (the constants code uses) at their ids, every
/// other after them in the order read.
fn ordered<'a, T>(list: Vec<(&'static str, Entry<'a>)>, keys: &[(&str, T)], files: &str) -> Res<Vec<Ordered<'a>>> {
    for (i, (id, e)) in list.iter().enumerate() {
        if list[..i].iter().any(|(other, _)| other == id) {
            return Err(e.err(format!("{id:?} is defined twice")));
        }
    }
    let mut slots: Vec<Option<Ordered>> =
        list.into_iter().enumerate().map(|(pos, (id, entry))| Some(Ordered { id, pos, entry })).collect();
    let mut out = Vec::with_capacity(slots.len());
    for (name, _) in keys {
        let id = name.to_ascii_lowercase();
        let Some(slot) = slots.iter_mut().find(|s| s.as_ref().is_some_and(|o| o.id == id)) else {
            return Err(LoadError {
                file: files.to_owned(),
                line: 0,
                field: id,
                message: "missing: the game refers to it by this id".to_owned(),
            });
        };
        out.extend(slot.take());
    }
    out.extend(slots.into_iter().flatten());
    Ok(out)
}

fn index<T>(list: &[Ordered], make: impl Fn(u16) -> T) -> HashMap<&'static str, T> {
    list.iter().enumerate().map(|(i, o)| (o.id, make(i as u16))).collect()
}

struct Ids {
    defs: HashMap<&'static str, DefId>,
    upgrades: HashMap<&'static str, UpgradeId>,
    perks: HashMap<&'static str, PerkId>,
    weapons: HashMap<&'static str, Weapon>,
    abilities: HashMap<&'static str, Ability>,
}

fn bonus(v: &Val) -> Res<[i32; 4]> {
    let mut t = v.table()?;
    let b = [
        t.or("infantry", 0, Val::int)?,
        t.or("light", 0, Val::int)?,
        t.or("heavy", 0, Val::int)?,
        t.or("structure", 0, Val::int)?,
    ];
    t.done()?;
    Ok(b)
}

/// A weapon: a name from `weapons.toml`, or a table.
fn weapon(v: &Val, ids: &Ids) -> Res<Weapon> {
    match v.v.get_ref() {
        DeValue::String(_) => v.lookup(&ids.weapons, "weapon"),
        _ => weapon_table(v.table()?),
    }
}

fn weapon_table(mut t: Entry) -> Res<Weapon> {
    let w = Weapon {
        damage: t.need("damage", Val::int)?,
        bonus: t.or("bonus", [0; 4], bonus)?,
        range: t.need("range", Val::dist)?,
        cooldown_ticks: t.need("cooldown", Val::time)?,
        shell: t.get("shell", |v| {
            let mut s = v.table()?;
            let shell = Shell {
                radius: s.need("radius", Val::dist)?,
                speed: s.need("speed", Val::speed)?,
                min_range: s.need("min_range", Val::dist)?,
            };
            s.done()?;
            Ok(shell)
        })?,
        targets: t.or("targets", Targets::Ground, |v| {
            v.pick(&[("ground", Targets::Ground), ("air", Targets::Air), ("both", Targets::Both)])
        })?,
        air_range: t.get("air_range", Val::dist)?,
        garrison_bonus: t.or("garrison_bonus", 0, Val::int)?,
        suicidal: t.or("suicidal", false, Val::bool)?,
        splash: t.or("splash", Fx::ZERO, Val::dist)?,
    };
    t.done()?;
    Ok(w)
}

fn blast(v: &Val) -> Res<Blast> {
    let mut t = v.table()?;
    let mut b = Blast {
        damage: t.need("damage", Val::int)?,
        bonus: t.or("bonus", [0; 4], bonus)?,
        radius: t.need("radius", Val::dist)?,
        cloud_radius: Fx::ZERO,
        cloud_damage: 0,
        cloud_ticks: 0,
    };
    if let Some((radius, damage, ticks)) = t.get("cloud", cloud)? {
        (b.cloud_radius, b.cloud_damage, b.cloud_ticks) = (radius, damage, ticks);
    }
    t.done()?;
    Ok(b)
}

/// A lingering cloud: its radius, damage a second and how long it lasts.
fn cloud(v: &Val) -> Res<(Fx, i32, u32)> {
    let mut t = v.table()?;
    let c = (t.need("radius", Val::dist)?, t.need("damage", Val::int)?, t.need("duration", Val::time)?);
    t.done()?;
    Ok(c)
}

fn status_effect(v: &Val) -> Res<StatusEffect> {
    let mut t = v.table()?;
    let s = StatusEffect {
        kind: t.need("status", Val::status)?,
        percent: t.or("percent", 0, Val::int)?,
        ticks: t.need("duration", Val::time)?,
    };
    t.done()?;
    Ok(s)
}

/// Up to two `{ status, percent }` entries.
fn statuses(v: &Val) -> Res<[Option<(StatusKind, i32)>; 2]> {
    let items = v.array()?;
    if items.len() > 2 {
        return Err(v.err("at most two statuses"));
    }
    let mut out = [None; 2];
    for (o, item) in out.iter_mut().zip(&items) {
        let mut t = item.table()?;
        *o = Some((t.need("status", Val::status)?, t.or("percent", 0, Val::int)?));
        t.done()?;
    }
    Ok(out)
}

/// An area effect's fields, read from `t` (a power's own table, or an ability's `field`).
fn field(t: &mut Entry) -> Res<Field> {
    Ok(Field {
        radius: t.need("radius", Val::dist)?,
        delay_ticks: t.or("delay", 0, Val::time)?,
        damage: t.or("damage", 0, Val::int)?,
        hp: t.or("hp", 0, Val::int)?,
        ranks: t.or("ranks", 0, Val::int)?,
        ticks: t.or("duration", 0, Val::time)?,
        allies: t.or("allies", [None; 2], statuses)?,
        enemies: t.or("enemies", [None; 2], statuses)?,
        vehicles: t.or("vehicles", false, Val::bool)?,
    })
}

fn ability(mut t: Entry, ids: &Ids) -> Res<Ability> {
    let name = t.need("name", Val::str)?;
    let cooldown_ticks = t.need("cooldown", Val::time)?;
    let kind = t.need("kind", |v| {
        v.pick(&[("boost", 0), ("detonate", 1), ("throw", 2), ("area", 3), ("shot", 4), ("trap", 5), ("sacrifice", 6)])
    })?;
    let kind = match kind {
        0 => {
            let items = t.need("statuses", Val::array)?;
            if items.len() > 2 {
                return Err(t.err("at most two statuses"));
            }
            let mut statuses = [None; 2];
            for (s, item) in statuses.iter_mut().zip(&items) {
                *s = Some(status_effect(item)?);
            }
            AbilityKind::Boost {
                statuses,
                radius: t.or("radius", Fx::ZERO, Val::dist)?,
                infantry: t.or("infantry", false, Val::bool)?,
                cloak_ticks: t.or("cloak", 0, Val::time)?,
            }
        }
        1 => AbilityKind::Detonate,
        2 => AbilityKind::Throw {
            range: t.need("range", Val::dist)?,
            blast: t.need("blast", blast)?,
            shells: t.need("shells", Val::int)?,
            every_ticks: t.need("every", Val::time)?,
            spread: t.need("spread", Val::dist)?,
            speed: t.need("speed", Val::speed)?,
        },
        3 => AbilityKind::Area {
            range: t.need("range", Val::dist)?,
            field: t.need("field", |v| {
                let mut f = v.table()?;
                let field = field(&mut f)?;
                f.done()?;
                Ok(field)
            })?,
        },
        4 => AbilityKind::Shot { weapon: t.need("weapon", |v| weapon(v, ids))? },
        5 => AbilityKind::Trap {
            range: t.need("range", Val::dist)?,
            radius: t.need("radius", Val::dist)?,
            damage: t.need("damage", Val::int)?,
        },
        _ => AbilityKind::Sacrifice {
            range: t.need("range", Val::dist)?,
            radius: t.need("radius", Val::dist)?,
            blast: t.need("blast", blast)?,
        },
    };
    t.done()?;
    Ok(Ability { name, kind, cooldown_ticks })
}

fn power(mut t: Entry, ids: &Ids) -> Res<Power> {
    let name = t.need("name", Val::str)?;
    let cooldown_ticks = t.need("recharge", Val::time)?;
    let first_ticks = t.or("first", cooldown_ticks, Val::time)?;
    let blind = t.or("blind", false, Val::bool)?;
    let requires = t.get("requires", |v| v.def(ids))?;
    let kind = t.need("kind", |v| {
        v.pick(&[
            ("barrage", 0),
            ("field", 1),
            ("ambush", 2),
            ("airstrike", 3),
            ("recon", 4),
            ("missiles", 5),
            ("cloak", 6),
            ("sabotage", 7),
            ("decoys", 8),
        ])
    })?;
    let kind = match kind {
        0 => PowerKind::Barrage {
            shells: t.need("shells", Val::int)?,
            delay_ticks: t.need("delay", Val::time)?,
            every_ticks: t.need("every", Val::time)?,
            damage: t.need("damage", Val::int)?,
            bonus: t.or("bonus", [0; 4], bonus)?,
            radius: t.need("radius", Val::dist)?,
        },
        1 => PowerKind::Field(field(&mut t)?),
        2 => PowerKind::Ambush { def: t.need("unit", |v| v.def(ids))?, count: t.need("count", Val::int)? },
        3 => PowerKind::Airstrike {
            def: t.need("aircraft", |v| v.def(ids))?,
            jets: t.need("jets", Val::int)?,
            spread: t.or("spread", Fx::ZERO, Val::dist)?,
            bombs: t.need("bombs", Val::int)?,
            spacing: t.or("spacing", Fx::ZERO, Val::dist)?,
            damage: t.need("damage", Val::int)?,
            bonus: t.or("bonus", [0; 4], bonus)?,
            radius: t.need("radius", Val::dist)?,
        },
        4 => PowerKind::Recon { def: t.need("aircraft", |v| v.def(ids))? },
        5 => {
            let missiles = t.need("missiles", Val::int)?;
            let delay_ticks = t.need("delay", Val::time)?;
            let every_ticks = t.need("every", Val::time)?;
            let speed = t.need("speed", Val::speed)?;
            let spread = t.need("spread", Val::dist)?;
            let damage = t.need("damage", Val::int)?;
            let radius = t.need("radius", Val::dist)?;
            let (cloud_radius, cloud_damage, cloud_ticks) = t.or("cloud", (Fx::ZERO, 0, 0), cloud)?;
            PowerKind::Missiles {
                missiles,
                delay_ticks,
                every_ticks,
                speed,
                spread,
                damage,
                radius,
                cloud_radius,
                cloud_damage,
                cloud_ticks,
            }
        }
        6 => PowerKind::Cloak { radius: t.need("radius", Val::dist)?, ticks: t.need("duration", Val::time)? },
        7 => PowerKind::Sabotage { damage: t.need("damage", Val::int)? },
        _ => PowerKind::Decoys { radius: t.need("radius", Val::dist)?, ticks: t.need("duration", Val::time)? },
    };
    t.done()?;
    Ok(Power { name, kind, first_ticks, cooldown_ticks, blind, requires })
}

fn garrison(v: &Val) -> Res<Garrison> {
    let mut t = v.table()?;
    let g = Garrison {
        capacity: t.need("capacity", Val::int)?,
        fires: t.or("fires", false, Val::bool)?,
        range_bonus: t.or("range_bonus", Fx::ZERO, Val::dist)?,
        heal_ticks: t.or("heal_time", 0, Val::time)?,
    };
    t.done()?;
    Ok(g)
}

fn def(mut t: Entry, ids: &Ids, unit: bool, ranks: Option<([u32; 3], i32)>) -> Res<Def> {
    let name = t.need("name", Val::str)?;
    let faction = t.get("faction", Val::faction)?;
    let class = if unit {
        Class::Unit {
            speed: t.need("speed", Val::speed)?,
            constructor: t.or("constructor", false, Val::bool)?,
            collector: t.get("collector", |v| {
                let mut c = v.table()?;
                let spec = CollectorSpec {
                    capacity: c.need("capacity", Val::int)?,
                    load_ticks: c.need("load_time", Val::time)?,
                    unload_ticks: c.need("unload_time", Val::time)?,
                };
                c.done()?;
                Ok(spec)
            })?,
            weapon: t.get("weapon", |v| weapon(v, ids))?,
            flies: t.or("flies", false, Val::bool)?,
        }
    } else {
        let role = t.need("role", |v| {
            v.pick(&[
                ("command_center", Some(BuildingRole::CommandCenter)),
                ("supply_center", Some(BuildingRole::SupplyCenter { collector_cap: 0 })),
                ("power_plant", Some(BuildingRole::PowerPlant)),
                ("oil_derrick", Some(BuildingRole::OilDerrick { credits_per_second: 0 })),
                ("factory", Some(BuildingRole::Factory)),
                ("lab", Some(BuildingRole::Lab)),
                ("civilian", Some(BuildingRole::Civilian)),
                ("defense", Some(BuildingRole::Defense)),
                ("superweapon", Some(BuildingRole::Superweapon)),
                ("bunker", Some(BuildingRole::Bunker)),
                ("tunnel", Some(BuildingRole::Tunnel)),
                // Map features: placed by the map, never built or owned.
                ("supply_pile", None),
                ("oil_well", None),
            ])
            .map(|role| (role, v.str().ok() == Some("supply_pile")))
        })?;
        match role {
            (None, true) => Class::SupplyPile,
            (None, false) => Class::OilWell,
            (Some(role), _) => {
                let role = match role {
                    BuildingRole::SupplyCenter { .. } => {
                        BuildingRole::SupplyCenter { collector_cap: t.need("collector_cap", Val::int)? }
                    }
                    BuildingRole::OilDerrick { .. } => {
                        BuildingRole::OilDerrick { credits_per_second: t.need("credits_per_second", Val::int)? }
                    }
                    role => role,
                };
                Class::Building {
                    power: t.or("power", 0, Val::int)?,
                    power_radius: t.or("power_radius", Fx::ZERO, Val::dist)?,
                    role,
                    weapon: t.get("weapon", |v| weapon(v, ids))?,
                }
            }
        }
    };
    let veterancy = t.get("ranks", |v| {
        let mut r = v.table()?;
        let (at, idle_regen) = match ranks {
            Some((at, regen)) => (r.or("at", at, |v| v.ints())?, r.or("idle_regen", regen, Val::int)?),
            None => (r.need("at", |v| v.ints())?, r.need("idle_regen", Val::int)?),
        };
        let vet = Veterancy {
            experience: at,
            health: r.need("health", Val::int)?,
            idle_regen,
            cooldown_percent: r.need("cooldown_percent", Val::int)?,
        };
        r.done()?;
        Ok(vet)
    })?;
    let abilities = t.or("abilities", &[][..], |v| {
        let list = v.array()?;
        if list.len() > MAX_ABILITIES {
            return Err(v.err(format!("at most {MAX_ABILITIES} abilities")));
        }
        Ok(leak(list.iter().map(|a| a.lookup(&ids.abilities, "ability")).collect::<Res<_>>()?))
    })?;
    let d = Def {
        name,
        faction,
        class,
        hp: t.need("hp", Val::int)?,
        armour: if unit { t.need("armour", Val::armour)? } else { t.or("armour", Armour::Structure, Val::armour)? },
        cost: Cost { credits: t.or("cost", 0, Val::int)? },
        build_ticks: t.or("build_time", 0, Val::time)?,
        radius: t.need("radius", Val::dist)?,
        produces: t.or("produces", &[][..], |v| v.defs(ids))?,
        builds: t.or("builds", &[][..], |v| v.defs(ids))?,
        deploy: t.get("deploy", |v| {
            let mut d = v.table()?;
            let deploy = Deploy { weapon: d.need("weapon", |v| weapon(v, ids))?, ticks: d.need("time", Val::time)? };
            d.done()?;
            Ok(deploy)
        })?,
        researches: t.or("researches", &[][..], |v| {
            Ok(leak(v.array()?.iter().map(|u| u.lookup(&ids.upgrades, "upgrade")).collect::<Res<_>>()?))
        })?,
        experience: t.or("experience", u32::from(unit), Val::int)?,
        veterancy,
        garrison: t.get("garrison", garrison)?,
        stealth: t.or("stealth", false, Val::bool)?,
        detection: t.or("detection", Fx::ZERO, Val::dist)?,
        becomes: t.get("becomes", |v| v.def(ids))?,
        special: t.get("special", |v| {
            let mut s = v.table()?;
            let special = Special {
                name: s.need("name", Val::str)?,
                weapon: s.need("weapon", |v| weapon(v, ids))?,
                cooldown_ticks: s.need("cooldown", Val::time)?,
                blast: s.get("blast", blast)?,
            };
            s.done()?;
            Ok(special)
        })?,
        abilities,
        unique: t.or("unique", false, Val::bool)?,
        blast: t.get("blast", blast)?,
    };
    t.done()?;
    Ok(d)
}

/// `{ unit, effect, ... }`: what an upgrade or perk changes for one unit or building.
fn effects(v: &Val, ids: &Ids) -> Res<&'static [(DefId, Effect)]> {
    let mut out = Vec::new();
    for item in v.array()? {
        let mut t = item.table()?;
        let def = t.need("unit", |v| v.def(ids))?;
        let kind = t.need("effect", |v| {
            v.pick(&[
                ("damage", 0),
                ("shell_damage", 1),
                ("cooldown", 2),
                ("toughness", 3),
                ("health", 4),
                ("repair", 5),
                ("experience", 6),
            ])
        })?;
        let effect = match kind {
            0 => Effect::Damage { percent: t.need("percent", Val::int)?, against: t.get("against", Val::armour)? },
            1 => Effect::ShellDamage { percent: t.need("percent", Val::int)? },
            2 => Effect::Cooldown { percent: t.need("percent", Val::int)? },
            3 => Effect::Toughness { percent: t.need("percent", Val::int)? },
            4 => Effect::Health { add: t.need("add", Val::int)? },
            5 => Effect::Repair { hp: t.need("hp", Val::int)?, calm_ticks: t.need("calm", Val::time)? },
            _ => Effect::Experience { percent: t.need("percent", Val::int)? },
        };
        t.done()?;
        out.push((def, effect));
    }
    Ok(leak(out))
}

/// `{ unit, cost_percent | build_time_percent }`.
fn modifiers(v: &Val, ids: &Ids) -> Res<&'static [Modifier]> {
    let mut out = Vec::new();
    for item in v.array()? {
        let mut t = item.table()?;
        let def = t.need("unit", |v| v.def(ids))?;
        match (t.get("cost_percent", Val::int)?, t.get("build_time_percent", Val::int)?) {
            (Some(percent), None) => out.push(Modifier::Cost { def, percent }),
            (None, Some(percent)) => out.push(Modifier::BuildTime { def, percent }),
            _ => return Err(t.err("give one of cost_percent or build_time_percent")),
        }
        t.done()?;
    }
    Ok(leak(out))
}

fn upgrade(mut t: Entry, ids: &Ids) -> Res<Upgrade> {
    let u = Upgrade {
        name: t.need("name", Val::str)?,
        cost: Cost { credits: t.need("cost", Val::int)? },
        research_ticks: t.need("research_time", Val::time)?,
        effects: t.or("effects", &[][..], |v| effects(v, ids))?,
    };
    t.done()?;
    Ok(u)
}

fn perk(mut t: Entry, ids: &Ids) -> Res<Perk> {
    let p = Perk {
        name: t.need("name", Val::str)?,
        description: t.need("description", Val::str)?,
        rank: t.need("rank", Val::int)?,
        price: t.need("price", Val::int)?,
        requires: t.get("requires", |v| v.lookup(&ids.perks, "perk"))?,
        effects: t.or("effects", &[][..], |v| effects(v, ids))?,
        modifiers: t.or("modifiers", &[][..], |v| modifiers(v, ids))?,
        power: t.get("power_tuning", |v| {
            let mut p = v.table()?;
            let tuning = match (p.get("cooldown_percent", Val::int)?, p.get("strength_percent", Val::int)?) {
                (Some(percent), None) => PowerTuning::Cooldown { percent },
                (None, Some(percent)) => PowerTuning::Strength { percent },
                _ => return Err(p.err("give one of cooldown_percent or strength_percent")),
            };
            p.done()?;
            Ok(tuning)
        })?,
        adds_power: t.get("adds_power", |v| power(v.table()?, ids))?,
    };
    t.done()?;
    Ok(p)
}

fn general(mut t: Entry, ids: &Ids) -> Res<General> {
    let g = General {
        name: t.need("name", Val::str)?,
        faction: t.need("faction", Val::faction)?,
        modifiers: t.or("modifiers", &[][..], |v| modifiers(v, ids))?,
        power: t.get("power", |v| power(v.table()?, ids))?,
        power_levels: t.or("power_levels", &[][..], |v| {
            Ok(leak(v.array()?.iter().map(|p| power(p.table()?, ids)).collect::<Res<_>>()?))
        })?,
        units: t.or("units", &[][..], |v| {
            let mut out = Vec::new();
            for item in v.array()? {
                let mut u = item.table()?;
                out.push(GeneralUnit {
                    unit: u.need("unit", |v| v.def(ids))?,
                    replaces: u.get("replaces", |v| v.def(ids))?,
                    at: u.need("at", |v| v.def(ids))?,
                });
                u.done()?;
            }
            Ok(leak(out))
        })?,
        perks: t.or("perks", &[][..], |v| {
            Ok(leak(v.array()?.iter().map(|p| p.lookup(&ids.perks, "perk")).collect::<Res<_>>()?))
        })?,
    };
    t.done()?;
    Ok(g)
}
