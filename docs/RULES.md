# Rules files

The game's units, buildings, weapons, abilities, upgrades, generals, perks and support powers are data: eight TOML files in `crates/g2-rules/data/`. They are compiled into every binary (`include_str!`), so a release needs nothing beside it, and parsed once at startup by `crates/g2-rules/src/data.rs` into the `Rules` the simulation reads. Parsing all eight takes about a millisecond. The campaign (`crates/g2-rules/src/missions.rs`) and the built-in Crater Ridge map stay Rust.

| File | Holds |
|---|---|
| `weapons.toml` | named weapons several units share, one `[name]` table each |
| `abilities.toml` | unit abilities, one `[name]` table each |
| `buildings.toml` | `[[building]]`: buildings, map features (supply pile, oil well), civilian buildings |
| `units.toml` | `[[unit]]`, and `ranks`, the veterancy rules every ranked unit shares |
| `powers.toml` | `[[support]]`: each faction's support powers, in slot order |
| `upgrades.toml` | `[[upgrade]]` |
| `perks.toml` | `[[perk]]` |
| `generals.toml` | `[[general]]`, each with its power and power levels |

Comments (`#`) say where a number comes from: `src:` marks a value from the archived reference material, everything else is a placeholder, often with how it was measured.

## Ids

Every unit, building, upgrade, perk and general has an `id` (`eu_recon`, `hollow_point`, `bd_double_shifts`, `bulldog`). Entries refer to each other by id (`produces = ["eu_dozer"]`, `requires = "bd_protector_i"`), in any order and across files. The game's own code names many of them too, through the constants in `crates/g2-rules/src/lib.rs` (`ids::EU_RECON` is `eu_recon`): each of those must be present, and keeps its number whatever the file order. An entry with a new id goes after them. Weapons and abilities are referred to by their table name (`weapon = "rifles"`, `abilities = ["nitro"]`).

Names (`name`) are what the player sees. The game looks a few up by name too (`"EU Howitzer"` in the client's animation code), so renaming one may need a code change.

## Units of measure

| Kind | Written as | Read as |
|---|---|---|
| distance, radius, range | world units: `25`, `3.5` | exact fixed point (16 fractional bits) |
| speed | world units per second: `9`, `45` | per tick, `units / 30`, exact fixed point |
| time | seconds: `6`, `0.9`, `24.44`; or ticks: `"1 tick"`, `"4 ticks"` | ticks (30 a second), rounded down |
| damage, hit points, credits | whole numbers | |
| percent | whole numbers: `125` is 125% of normal | |

Decimals are read as exact ratios (`3.5` is 7/2), never as floats, so a value gives the same bits on every machine and the simulation stays deterministic. Exponents (`1e3`), `inf` and `nan` are refused. Damage `bonus` tables list extra damage per armour class (`infantry`, `light`, `heavy`, `structure`); a class left out gets 0, and a bonus may be negative.

## Fields

A field left out takes the default shown; one without a default is required. A field the reader doesn't know is an error, so a misspelt name doesn't pass silently.

**Weapon** (in `weapons.toml`, or in place: `weapon = { damage = 46, range = 45, cooldown = 2.25 }`): `damage`, `range`, `cooldown` (time between shots), `bonus` ({}), `targets` (`"ground"`; `"air"`, `"both"`), `air_range` (the range against aircraft, else `range`), `shell` (an artillery shell: `{ radius, speed, min_range }`), `garrison_bonus` (0, extra damage to a building with units inside), `suicidal` (false: firing destroys the unit, its `blast` deals the damage), `splash` (0: radius hit around a direct-fire target).

**Blast** (`blast` of a unit, special, ability): `damage`, `radius`, `bonus` ({}), `cloud` (none: `{ radius, damage, duration }`, damage a second).

**Building**: `id`, `name`, `role`, `hp`, `radius`, `faction` (none), `armour` (`"structure"`), `cost` (0), `build_time` (0), `power` (0: positive supplies, negative draws), `power_radius` (0), `produces` ([]), `researches` ([] upgrade ids), `detection` (0, reveals stealth within it), `garrison` (none: `{ capacity, fires, range_bonus, heal_time }`), `weapon` (none, a defence's). Roles: `command_center`, `supply_center` (with `collector_cap`), `power_plant`, `oil_derrick` (with `credits_per_second`), `factory`, `lab`, `civilian`, `defense`, `superweapon`, `bunker`, `tunnel`, and the map features `supply_pile` and `oil_well`.

**Unit**: `id`, `name`, `hp`, `armour`, `radius`, `speed`, `faction` (none), `cost` (0), `build_time` (0), `flies` (false), `constructor` (false), `builds` ([] building ids), `collector` (none: `{ capacity, load_time, unload_time }`), `weapon` (none), `deploy` (none: `{ time, weapon }`, set up to fire this instead), `special` (none: `{ name, cooldown, weapon, blast }`, one aimed shot on command; with a `blast` it is a projectile bursting where the target stood), `abilities` ([], at most three), `ranks` (none: `{ health, cooldown_percent }`, plus `at` and `idle_regen` to differ from the file's `ranks`), `experience` (1, given to its killer), `garrison`, `becomes` (none, a def id it turns into), `stealth` (false), `detection` (0), `unique` (false: one per player, a hero), `blast` (none, set off where it dies).

**Ability**: `name`, `kind`, `cooldown`, and by kind: `boost` (`statuses`: up to two `{ status, percent, duration }`; `radius` 0, `infantry` false, `cloak` 0), `detonate`, `throw` (`range`, `blast`, `shells`, `every`, `spread`, `speed`), `area` (`range`, `field`: a power field's fields), `shot` (`weapon`), `trap` (`range`, `radius`, `damage`), `sacrifice` (`range`, `radius`, `blast`). Statuses: `toughness`, `damage`, `speed`, `sight`, `stun`.

**Power** (`[[support]]` with a `faction`, a general's `power` and `power_levels`, a perk's `adds_power`): `name`, `kind`, `recharge`, `first` (`recharge`: the wait for the first use), `blind` (false: may be aimed into the fog), `requires` (none, a building id that launches it), and by kind: `barrage` (`shells`, `delay`, `every`, `damage`, `bonus`, `radius`), `field` (`radius`, `delay`, `damage`, `hp`, `ranks`, `duration`, `allies`, `enemies`: up to two `{ status, percent }`, `vehicles`), `ambush` (`unit`, `count`), `airstrike` (`aircraft`, `jets`, `spread`, `bombs`, `spacing`, `damage`, `bonus`, `radius`), `recon` (`aircraft`), `missiles` (`missiles`, `delay`, `every`, `speed`, `spread`, `damage`, `radius`, `cloud`), `cloak` (`radius`, `duration`), `sabotage` (`damage`), `decoys` (`radius`, `duration`).

**Upgrade**: `id`, `name`, `cost`, `research_time`, `effects`: `{ unit, effect, ... }` with `effect` one of `damage` (`percent`, `against` an armour class or all), `shell_damage` (`percent`), `cooldown` (`percent`), `toughness` (`percent`), `health` (`add`), `repair` (`hp` a second, after `calm` without a hit), `experience` (`percent`).

**Perk**: `id`, `name`, `description`, `rank` (1-5), `price` (experience), `requires` (none, a perk id), `effects` (as an upgrade's), `modifiers` (`{ unit, cost_percent }` or `{ unit, build_time_percent }`), `power_tuning` (`{ cooldown_percent }` or `{ strength_percent }`, on the general's power), `adds_power` (a power).

**General**: `id`, `name`, `faction`, `modifiers`, `power`, `power_levels` (from general ranks 3 and 5), `units` (`{ unit, at, replaces }`: offered by `at`, in place of `replaces` if given), `perks` (perk ids).

## Modding

Copy the files you want to change into a folder, edit them, and point `G2_RULES_DIR` at it:

```
mkdir mymod && cp crates/g2-rules/data/units.toml mymod/
G2_RULES_DIR=$PWD/mymod cargo run -p g2-client --release
```

Files missing from the folder come from the built-in set. A bad file stops the game at startup with its name, line and field: `mymod/units.toml:212: eu_recon.hp: expected a whole number`.

Rules decide the match, so both machines of a network game must run the same ones. The lobby compares a hash of the rules (`build_id` in `crates/g2-sim/src/net.rs`): it covers every value in these files, so a modded game refuses to play an unmodded one, or one with another mod. Replays carry no rules, so a replay recorded under a mod replays correctly only under the same mod.
