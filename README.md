# generals2

Open-source recreation of the cancelled *Command & Conquer: Generals 2*, as a new engine written in Rust. Not affiliated with or endorsed by EA. All trademarks are the property of their respective owners.

The engine needs no game files. EA's GPL release of *Generals/Zero Hour* in `vendor/` is reading material only; nothing from it is ported or copied into `crates/`. All art is original (placeholder primitives for now).

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
- **Built-in AI** that plays the economy. It runs inside the sim and issues ordinary commands, so AI matches replay exactly.
- **Replays**: setup + human commands + a state hash every second. `g2-tools verify` re-simulates a replay and names the first tick that diverges. A committed golden replay fails the test suite on any unintended behaviour change.
- **Bevy 0.20 client** (`crates/g2-client`):
  - RTS camera, selection, and context orders (move, gather, help build).
  - Build placement with validity preview (derricks snap to wells).
  - Production hotkeys, a HUD, pause and ×1–×8 speed.
  - Interpolated movement between sim ticks.
  - `--record FILE` saves the match as a replay on exit, for `g2-tools verify`.
  - A headless `--screenshot` mode that renders one frame and checks that every sim entity has exactly one mesh at its sim position.

Not yet:

- Combat, fog of war, pathfinding around obstacles.
- The generals/perk system, the Onslaught PvE mode, networking, real art, audio.
- The **"base proxy" rule**: Wikipedia says a faction "must have 'captured' a resource stash before letting its harvesters in". The archived sources don't say how capture works, so for now any player can gather from any pile.
- Rules are const Rust tables, not data files.

![APA vs GLA after 5 minutes, AI vs AI, rendered headless](docs/img/m1-apa-vs-gla-5min.png)

*Above: APA (red) vs GLA (sand) after 5 minutes, AI vs AI. Rendered with `--screenshot` on Mesa llvmpipe (CPU Vulkan).*

**Verification status.**

- The headless frame above was rendered. Its HUD numbers match `g2-tools record` for the same match, and the mirror check passed: 31 sim entities, 31 meshes, 0 misplaced.
- A test drives the real Bevy schedule without a renderer: orders between frames, a ×4 burst, then a saved replay. That replay verifies to the same final hash, which covers the client's command path end to end.
- The **interactive** window (mouse, keyboard, camera) has not been exercised. The build host has no display, and its amdgpu device (`c7:00.0`) was already failing before this work started, according to the kernel log.
- To render on the CPU without touching a GPU:

  ```
  VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan cargo run -p g2-client --release -- --screenshot shot.png
  ```

## Layout

```
crates/g2-sim      deterministic simulation (no deps): fixed point, RNG, entities, economy, AI, replays
crates/g2-rules    faction/unit/building tables, the built-in map; economy + determinism tests
crates/g2-client   Bevy 0.20 app: rendering, camera, input, HUD, headless screenshot
crates/g2-tools    CLI: verify / record / bench / summary
archive/           research material (manifests committed, media local-only), see archive/README.md
vendor/            upstream source as git submodules (reference only), see vendor/README.md
the local field notes   annotated index of everything archived
```

## Build and run

Rust 1.99+.

```
cargo test --workspace                               # unit, economy, determinism, golden replay
cargo run -p g2-client --release                     # play EU vs GLA AI
cargo run -p g2-client --release -- --faction gla --enemy apa --seed 4
cargo run -p g2-client --release -- --spectate       # AI vs AI
cargo run -p g2-client --release -- --record my.g2rp # then: cargo run -p g2-tools --release -- verify my.g2rp
cargo run -p g2-client --release -- --screenshot shot.png --ticks 1800   # on this host, prefix the lavapipe env above
cargo run -p g2-tools --release -- bench
cargo run -p g2-tools --release -- record match.g2rp --factions apa,gla --ticks 18000
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

`vendor/` contains GPL-3.0 code; the EA release carries GPL §7 additional terms (no trademark use, no claims of EA affiliation, modifications must be marked). `archive/` bulk data is local-only and never committed or redistributed. `archive/docs/` (wiki text, press pages, concept art) **is** committed for research, so this repository must stay private, or that directory must be removed from history, before any public push.
