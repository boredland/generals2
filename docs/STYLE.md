# Visual style

Target mood: the 2013 alpha gameplay footage of the cancelled game: a warm, hard-lit military RTS seen from a steep angle, on dry savanna ground, with a spotlit centre. We match the **mood and readability**, not the content. Every model, texture and UI element is designed here and generated procedurally; no shapes, icons or layouts are traced from the footage.

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
- Warm, contrasty grade (temperature up, saturation boost, deep brown shadows), plus light distance haze. Exposure is set so the lit centre of a base frame lands near the footage's (about 120-140 sRGB luminance on a centre crop).
- Screen-space ambient occlusion grounds buildings and units and darkens model creases; FXAA smooths edges (MSAA is off for SSAO).

## Terrain (`terrain.rs`)

- Dry savanna in warm brown-ochre (the footage's open ground measures about sRGB 135/103/62), with olive only in scattered patches, bare dirt, dark speckle and noise at several scales. A tiling procedural detail texture adds dark tuft clumps, lit blades and grit, so the ground is busy at every zoom.
- Light worn **dirt roads** link the bases, supply piles and oil wells, so the economy routes are readable at a glance.
- Acacias standing apart: a slender forked trunk under flat clumped crowns of lit and shaded lumps, casting wide broken shadows; low multi-lump bushes; boulders. Kept off roads, base areas and resources.
- Dark rocky ridges frame the playable area; the vignette takes the edges to black.

## Models (`models.rs`)

Requirements: you can tell a faction from its silhouette and colour, models stay inside their sim footprint, and roofs are busy, because the camera mostly sees roofs. Buildings are layered industrial structures with three or more height levels: stepped roof tiers, rails and catwalks along roof edges, pipe runs, ribbed and seamed walls, recessed bays, corner trims, masts and dishes. Vehicles have tracks with road wheels and skirts (or wheels with fenders), a turret with mantlet and barrel, and stowage. Budget: at most 120 parts per building and 30 per unit (tests enforce both, and the footprint).

- **EU**: warm mid-grey industrial panels (near-white blows out in the sun), orange hazard trims, teal team accents.
- **APA**: dark steel, red team accents, tiered rooftops.
- **GLA**: improvised, rust-brown. Sandbag rings, scrap sheets, crate and barrel yards, tent camps, and a lookout post on the supply stash; sand team colour.
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

## Budget

The headless screenshot mode prints the mesh count and the average frame time over 30 frames. On the build host's integrated GPU at 1080p the densest original map (`zh:Red Rock`, about 35,000 meshes, mostly scenery) runs at 9-13 ms per frame; a typical base view at 720p at about 8 ms.
