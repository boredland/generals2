# Visual style

Target mood: the 2013 alpha gameplay footage of the cancelled game (see `the local field notes`): a warm, hard-lit military RTS seen from a steep angle, on dry savanna ground, with a spotlit centre. We match the **mood and readability**, not the content. Every model, texture and UI element is designed here and generated procedurally; no shapes, icons or layouts are traced from the footage.

Reference footage (local-only):

- `archive/videos/alpha/…[dtl2Q_tUDDQ].mkv`: 1080p closed-alpha gameplay, 24 Aug 2013 (primary mood reference).
- `archive/videos/wiki/…[A9g_pat78eA].mkv`, `archive/videos/ia/tweakers008132/`, `archive/videos/sweep/…[LuHGrw0AEl8].mkv`: 2013 stage and live demos.

## Camera (`look.rs`, `camera.rs`)

- Steep three-quarter view, about 57° below the horizon, narrow 32° field of view. Buildings read almost isometric.
- Close default zoom: a command center spans about a fifth of the screen width. Zoom range is narrow.
- Fixed default yaw, with the sun at the upper left of the screen.

## Light and grade (`look.rs`)

- Strong, warm afternoon sun. Shadows are crisp and fall toward the lower right. They are a readability tool: they show height and separate units from the ground.
- Warm ambient light, so shaded sides stay legible.
- **Spotlight vignette**: the corners fall off toward black and the action stays bright. This is the single strongest mood cue.
- Warm grade (temperature up, slight saturation boost), plus light distance haze.

## Terrain (`terrain.rs`)

- Dry savanna. Mostly ochre and olive grass, with patches of bare dirt and noise at several scales. A tiling procedural detail texture adds grass tufts and pebbles at close zoom.
- Light worn **dirt roads** link the bases, supply piles and oil wells, so the economy routes are readable at a glance.
- Scattered trees with flat canopies, bushes and boulders, kept off roads, base areas and resources.
- Dark rocky ridges frame the playable area; the vignette takes the edges to black.

## Models (`models.rs`)

Requirements: you can tell a faction from its silhouette and colour, models stay inside their sim footprint (a test enforces this), and roofs are busy, because the camera mostly sees roofs.

- **EU**: light grey industrial panels, hazard-striped roof edges, teal team accents.
- **APA**: dark steel, red team accents, tiered rooftops.
- **GLA**: improvised. Scrap walls, crate and barrel yards, and a lookout post on the supply stash; sand team colour.
- **Construction**: a coloured scaffold frame with crane arms surrounds the foundation, and the building grows upward inside it as the sim's construction progresses.
- **Resources**: supply piles are stacks of mixed crates on a pallet that shrink as they deplete. Oil wells are black pools with a wellhead.
- **Collectors**: the EU collector flies (sourced), as an original ducted-fan lifter with a cargo cradle. The APA collector is a supply truck. GLA workers are on foot and also build.
- Constructors are yellow and black.

## HUD (`hud.rs`)

Requirements: every element shows live sim data, nothing is decorative, there are no EA trademarks in the UI, and the panels are our own (gunmetal, teal accents).

- Conventional RTS zones: minimap bottom-left, selection and production bottom-right, clock top-centre.
- **Bottom-left panel**: power used/supplied, oil and credits above a minimap with team-coloured units and buildings and neutral resource dots.
- **Bottom-right panel**: selection name, health and status (construction %, power, cargo), a training row that only appears while something is queued (with a progress bar), and a numbered 3×3 grid showing what the hotkeys 1–9 build or train.
- **Top centre**: a compact match bar with both factions' tags around the clock. Top left: menu label. Top right: neutral project label.
- **World overlays**: `COLLECTORS n/cap` over own supply centers (red at the faction cap), and health bars over the selection and damaged entities.
- Not shown until the systems exist: generals and perks, abilities, experience.
