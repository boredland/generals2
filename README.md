# generals2

Open-source recreation of the cancelled *Command & Conquer: Generals 2*, as a new engine written in Rust. Not affiliated with or endorsed by EA. All trademarks are the property of their respective owners.

The engine needs no game files. EA's GPL release of *Generals/Zero Hour* in `vendor/` is reference for the engine: no code from it is ported or copied into `crates/`. The one runtime use of `vendor/` is map data: the original Generals and Zero Hour **maps** can be played, read at runtime from EA's GPL map sources (`vendor/ea/CnC_Modding_Support`, a git submodule) and never copied into this repository. All art is original and procedural: meshes built from primitives, generated terrain and textures, our own HUD design. The archived 2013 footage sets the target mood (warm hard light, steep camera, savanna ground); `docs/STYLE.md` describes it as requirements, not as elements to copy.

## Status: milestone 1, economy slice

What works:

- **Deterministic simulation** (`crates/g2-sim`): 30 ticks/s like SAGE, 48.16 fixed point, PCG32, a generational entity arena, a per-tick state hash. No floats in state, no dependencies, no IO. About 100,000 ticks/s single-threaded (3,000× real time) for a 2-player AI match.
- **Generals 2 economy**, from the archived sources:
  - Credits from supply piles, carried by collectors to a supply center.
  - Collector cap per supply center: **EU 3, APA 5, GLA 8**.
  - Oil from derricks, which can only sit on oil wells and pay out until the well is dry.
  - Local power: plants power consumers within a radius. Unpowered production runs at half speed. The GLA has no grid.
  - Constructors place foundations that must be worked on, production queues with full refunds on cancel, and surrender/defeat.
- **Three factions** (`crates/g2-rules`): EU, APA and GLA. Command center, constructor, supply center, collector, power plant and oil derrick each. Sourced numbers are tagged `// src:`, everything else is a placeholder.
- **Generals as data**: a player may pick a general of their faction; its modifiers change the cost and build time of specific units and buildings (integer percent, deterministic, refunded at the price paid). One per faction so far: **Bulldog** (EU), **Taskmaster** (APA), **Junkyard** (GLA). The names are documented Generals 2 generals; their effects are placeholders, mod-style sub-generals (per-unit cost and time changes). The general is part of the state hash and of replays (format v3; v1/v2 still load), and replay files naming an unknown or wrong-faction general are refused with an error.
- **Built-in AI** that plays the economy. It runs inside the sim and issues ordinary commands, so AI matches replay exactly.
- **Pathfinding** (`crates/g2-sim/src/nav.rs`): units route around buildings, foundations, resources and, on imported maps, cliffs and water. A 2-unit grid; a cell is blocked when it lies within a static's radius plus a unit's radius, or overlaps terrain the original engine marks impassable (the map's own cliff bits, or for pre-v7 maps the engine's 9.8-unit height-span rule, and cells under a water area). Integer A* with fixed tie-breaking and line-of-sight smoothing; an unreachable goal routes to the closest reachable cell. A unit walks straight while the line is clear and plans only when it isn't. Routes are entity state and part of the hash. Buildings can't be placed on cliffs or water. Collectors don't push each other (two meeting head-on in a gap between buildings blocked each other for good), and a test checks that no AI collector of any faction stands still for 20 s on a gather run.
- **Replays**: setup + human commands + a state hash every second, plus a fingerprint of the map, so a replay whose map file has since changed is reported as such instead of as a divergence. `g2-tools verify` re-simulates a replay and names the first tick that diverges. A committed golden replay fails the test suite on any unintended behaviour change.
- **Bevy 0.20 client** (`crates/g2-client`):
  - RTS camera, selection, and context orders (move, gather, help build).
  - Build placement with validity preview (derricks snap to wells).
  - Production hotkeys, a HUD, pause and ×1–×8 speed. The build grid shows prices after your general's modifiers, and the match bar names both generals.
  - Collectors: cargo pips over your own collectors, a "+credits" pop-up over the supply center on every delivery (none are lost at ×8; a test compares them with the sim), and Tab cycles through your collectors.
  - Interpolated movement between sim ticks.
  - `--record FILE` saves the match as a replay on exit, for `g2-tools verify`.
  - A headless `--screenshot` mode that renders one frame and checks that every sim entity has exactly one model at its sim position. `--view base|map` picks the in-game camera or a whole-map overview.
- **Visuals** (`look.rs`, `terrain.rs`, `models.rs`, `hud.rs`): narrow-FOV steep camera, a low warm sun with cascaded shadows, AgX tonemapping with a warm grade, distance haze and a vignette. The terrain is procedural savanna with dirt roads, a tiling detail texture, trees, bushes and rocky rims. As in the 2013 footage, scenery grows right up to the bases: only the command center and resource spots start clear, and a building hides any scenery it covers while it stands. Each faction has its own procedural models: EU grey panels with hazard trim and a ducted-fan collector, APA dark steel with red accents, GLA scrap walls and crate yards. Construction sites have grey scaffolding, team-coloured netting and a tower crane. The HUD shows live data only: match bar, resources, a minimap with team dots and neutral resources, a selection panel with a training queue and a 3×3 production grid, plus collector counts and health bars in the world.
- **Original Generals / Zero Hour maps** (`crates/g2-maps`): a reader for the SAGE `.map` format (RefPack and zlib compression, the `CkMp` chunk container). It turns a map into a match: player starts, supply docks and piles with their retail amounts (boxes × 75 credits), and tech oil derricks as oil wells. All 174 official maps in `vendor/ea/CnC_Modding_Support` parse; 85 of them have two or more player starts and can host a skirmish. In the client the original relief, painted ground (grass, sand, rock, snow, paved), road network, water areas and tree, bush and rock positions are drawn in our own procedural style; models, the cursor and the camera follow the rendered ground. `g2-tools maps` lists them; `--map zh:Tournament Desert` picks one. Map names in replays are checked so they cannot name paths outside the map folder. Scale: 6 SAGE units = 1 world unit, so a retail command center matches ours.

Not yet:

- Combat, fog of war. Units still push through each other (except collectors, which pass through); there is no local avoidance.
- Generals' unique units, player powers and the per-general perk progression (rank, perk slots; archived patch preview #2). Only cost/time modifiers exist.
- The Onslaught PvE mode, networking, real art, audio.
- The AI doesn't rebuild a lost constructor. Nothing can destroy one yet, so it can't happen in play; it matters once combat exists.
- The **"base proxy" rule**: Wikipedia says a faction "must have 'captured' a resource stash before letting its harvesters in". The archived sources don't say how capture works, so for now any player can gather from any pile.
- Rules are const Rust tables, not data files.
- On imported maps **bridges are not walkable yet**: their width comes from the bridge models, which we don't have. On 5 of the 85 skirmish maps (Final Crusade, Heartland Shield in both games, Winding River, FloodedPlains) some starts are cut off from each other by water; every start still reaches its own supply. Slopes are walkable and buildings may stand on them; heights don't affect the sim. Retail tech buildings, civilian structures, cliff textures, blended ground edges and scripts are ignored. Oil wells are finite (10,000 oil each), unlike retail derricks. Water surfaces are flat fans over each area's outline, so a strongly concave lake can overhang its shore. Only 2-player matches are supported, even on 4-, 6- and 8-start maps.

![EU base at 3 minutes, in-game camera](docs/img/m1-eu-base-3min.png)

![APA vs GLA after 5 minutes, whole-map view](docs/img/m1-apa-vs-gla-5min.png)

*Above: an EU base at 3:00 (EU vs GLA AI), and APA (red) vs GLA (sand) after 5 minutes of AI vs AI with `--view map`. Both rendered headless with `--screenshot` on the integrated Radeon (RADV GFX1150).*

![Zero Hour's Tournament Desert, EU base at 3 minutes](docs/img/m2-zh-tournament-desert-base.png)

![Zero Hour's Winding River, whole-map view](docs/img/m2-zh-winding-river-map.png)

*Above: original maps, read from EA's map sources and drawn in this project's style: `--map "zh:Tournament Desert"` (base view) and `--map "zh:Winding River"` (`--view map`).*

**Verification status.**

- Both frames above were rendered on this host, and the mirror check passed for each (30/30 and 31/31 entities, none misplaced). Their HUD totals match `g2-tools record --seed 1` for the same matches: EU at 5,400 ticks shows `$2250`, oil 1045, power 40/210; APA at 9,000 ticks shows `$6500`, oil 2240, power 40/210.
- A test drives the real Bevy schedule without a renderer: orders between frames, a ×4 burst, then a saved replay. That replay verifies to the same final hash, which covers the client's command path end to end.
- Imported maps: all 174 official maps parse in a test; a recorded AI match on `zh:Tournament Desert` round-trips through a replay and verifies to the same hash, and a moved resource is caught as a changed map; terrain tests check that starts sit on the battlefield level, original scenery stays off bases and water, the ridge past the edge has no steps, and water surfaces stay inside the map; a test aims camera rays at 361 points on Defcon6's terraces and checks each lands within 0.5 units. Tournament Desert, Alpine Assault, Golden Oasis, Defcon6 and Winding River were rendered headless with the mirror check passing (models on terrain, none misplaced): Tournament Desert 37/37, Winding River 27/27 for the frames above.
- The **interactive** window (mouse, keyboard, camera) has not been exercised; the build host has no display. That now includes code added for imported maps that only the window runs: clicking onto hills (the ray march itself is unit-tested) and the camera easing up and down terrain while panning. The "+credits" pop-ups have never been seen either: screenshot mode steps the sim without the client loop, so none spawn. A test checks that every delivery reaches the pop-up queue at ×8; the cargo pips are visible in the base frame above.
- **GPU on the build host:** the discrete amdgpu device (`c7:00.0`, MI50) is wedged; opening it hangs the process in D state. Run every GPU command through `scripts/igpu-only/run.sh`. It hides the MI50's DRM nodes with an `LD_PRELOAD` shim, so only the integrated Radeon (`c8:00.0`, `renderD128`) is enumerated. strace confirms `renderD128` is the only device node opened:

  ```
  scripts/igpu-only/run.sh target/release/g2-client --screenshot shot.png --ticks 5400 --view base
  ```

- To render on the CPU instead (Mesa lavapipe):

  ```
  VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan cargo run -p g2-client --release -- --screenshot shot.png
  ```

## Layout

```
crates/g2-sim      deterministic simulation (no deps): fixed point, RNG, entities, economy, AI, replays
crates/g2-rules    faction/unit/building tables, the built-in map; economy + determinism tests
crates/g2-client   Bevy 0.20 app: look (camera/light/grade), terrain, models, view, input, HUD, headless screenshot
crates/g2-maps     reader for Generals / Zero Hour .map files: chunks, heights, textures, objects, water
crates/g2-tools    CLI: verify / record / bench / summary / maps
scripts/igpu-only/ LD_PRELOAD shim + wrapper that restricts Vulkan to the integrated GPU on the build host
archive/           research material, local-only and untracked (fetch scripts in scripts/), see the local field notes
vendor/            upstream source as git submodules (reference only), see vendor/README.md
the local field notes   annotated index of everything archived
docs/STYLE.md      visual target: mood and readability requirements
```

## Build and run

Rust 1.99+.

```
cargo test --workspace                               # unit, economy, determinism, golden replay
cargo run -p g2-client --release                     # play EU vs GLA AI
cargo run -p g2-client --release -- --faction gla --enemy apa --seed 4
cargo run -p g2-client --release -- --spectate       # AI vs AI
cargo run -p g2-tools --release -- maps             # list the original maps that can host a skirmish
cargo run -p g2-client --release -- --map "zh:Tournament Desert" --faction apa
cargo run -p g2-client --release -- --record my.g2rp # then: cargo run -p g2-tools --release -- verify my.g2rp
cargo run -p g2-client --release -- --screenshot shot.png --ticks 1800   # on this host, wrap with scripts/igpu-only/run.sh
cargo run -p g2-tools --release -- bench
cargo run -p g2-tools --release -- record match.g2rp --factions apa,gla --ticks 18000 --map "gen:Alpine Assault"
cargo run -p g2-tools --release -- verify match.g2rp
```

Bevy is built with an explicit **X11-only** feature list (see the root `Cargo.toml`), because the build host lacks Wayland development headers. On a host that has them, add `"wayland"` to the list.

## Controls

| Input | Action |
|---|---|
| Left click / drag | Select own units (click selects buildings too); Shift adds |
| Right click | Supply pile → gather; own foundation → help build; ground → move |
| Constructor selected: `1`–`9` | Choose a building; left click places (Shift repeats); Esc / right click cancels |
| Building selected: `1`–`9` | Queue production; Backspace cancels the last item |
| WASD / arrows / screen edge | Pan; mouse wheel zooms; Q/E rotate |
| Space | Pause; `+`/`-` change speed (×1–×8) |
| Tab | Select the next own collector and centre the camera on it |
| F10 | Surrender |

## Determinism rules for `g2-sim`

Every change to the simulation must keep replays bit-exact:

- No `f32`/`f64` in state or logic; use `Fx`. `Fx::to_f32` exists only for rendering.
- No `HashMap`/`HashSet` iteration, system time, threads or IO.
- Iterate entities in index order. Every new state field goes into `World::state_hash`.
- Intentional behaviour changes regenerate the golden replay: `G2_BLESS=1 cargo test -p g2-rules --test determinism golden`.

## Inspiration

[Strong Hold](https://yannickaaron.github.io/Strong-Hold/), a native Rust engine for *Stronghold Crusader*. We took the following ideas from it:

- A simulation crate with no dependencies, kept apart from the app.
- Checking every tick against recordings.
- Fast-forward.
- Bringing the engine, not the game.

The difference: Strong Hold reimplements an existing game and checks it against the original binary. Generals 2 never shipped, so we check against our own recorded replays.

## Legal

`vendor/` contains GPL-3.0 code; the EA release carries GPL §7 additional terms (no trademark use, no claims of EA affiliation, modifications must be marked). No code from it is in `crates/`; `crates/g2-maps` is our own reader written from the file format. Imported map data is read from `vendor/ea/CnC_Modding_Support` (or `$G2_MAPS_DIR`, for a binary run outside this tree) at runtime and never copied into the repository. Everything under `archive/` (videos, leaks, Wayback captures, wiki text, press pages, concept art, and their manifests) is third-party material kept local-only: it is gitignored, has never been in this repository's history, and is never redistributed. `the local field notes` links into it; those links resolve only in a local checkout that has run the fetch scripts.
