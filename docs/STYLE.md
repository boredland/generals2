# Visual style

Target mood: the 2013 alpha gameplay footage of the cancelled game: a warm, hard-lit military RTS seen from a steep angle, on dry savanna ground, with a spotlit centre. We match the **mood and readability**, not the content. Every model, texture and UI element is designed here and generated procedurally; no shapes, icons or layouts are traced from the footage. The one exception is the menus' layout (see Menus), by choice.

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

## Menus (`menu.rs`, `front.rs`)

The front end follows the alpha's as its skirmish footage shows it (`archive/videos/alpha/…[3tlbDYSUboc].mkv`, the first 25 s): the layout is recreated on purpose, the content is ours.

- A black screen under a centred, letter-spaced silver title and a small red build tag top right, where the alpha had "ALPHA PREVIEW". No EA or Command & Conquer marks.
- **Main**: a commander card (total XP), the general's portrait with a cycler, a big blue PLAY over a SKIRMISH / ONSLAUGHT mode bar, then LIVE / PROFILE / LEARN / QUIT tabs. Under them a banner with the general's loading-screen art and what this build holds (counted from the rules and maps), and the player's generals with rank and XP where the alpha had its social panel.
- **Lobby**: a map banner (the official map's own `.tga` preview, sharp at the banner's height over a dimmed stretched copy; Crater Ridge drawn from its map), the name bar as "Name (1v1)" opening the paged map list, TEAM 1 VS TEAM 2 player cards with portrait, faction mark, general, faction and the AI's EASY / NORMAL / ONSLAUGHT pills, match options in the right column, and a big green START BATTLE.
- Pause, controls and the end screen are centred dialogs; the full screens hang from the title.
- With the player's import, the alpha's Eurostile fonts, portraits, faction logos and loading-screen art are used; without it, the same layout falls back to bevy's font and faction-coloured tiles. Features the alpha's front end had and this game doesn't (currencies, friends, chat, store) are left out rather than faked.

## Combat effects (`fx.rs`)

Requirements: you can tell what fired from what you see, a kill reads bigger than a hit, and effects never touch the sim (wrecks and particles are not hashed; the shapes come from the spot and tick, so a replay draws the same ones). Each shot's look comes from the firing unit's weapon: machine guns, cannons, rockets, flamethrowers, toxin sprayers, lasers and railguns, artillery shells, and power strikes (bombs, barrages and missiles).

- Soft billboarded puffs on one procedural sprite: additive fire, flame, spark, toxin and plasma tracks that fade by darkening, and blended smoke, dust and toxin-haze tracks in mid tones picked against the graded ground. Every track's materials are made once and shared, so a particle is two handle clones.
- **Direct fire**: machine guns flash and send a burst of glowing dashes down the line (three for heavy guns, two for light); cannons flash, puff barrel smoke and send a hot slug; a rocket flies with a smoke trail; a flamethrower sends an orange stream widening toward the target; a toxin sprayer a green spray; a laser a beam with a hot core and a faint sheath, the railgun's blue-white with a vapour trail. Each ends in sparks, a puff and dust.
- **Shells and strikes**: a shell flies its arc with a thin smoke trail; a power's bombs and barrage shells drop steeply from high above each spot; missiles climb high and come down. The landing blast is a white flash, a fireball, a dust ring, smoke that rises and spreads, debris that settles on the ground and a point light. Toxin blasts are green with a haze, a nuke's blue-white, and napalm leaves fire burning on the ground.
- **Kills**: a fireball sized by what died (a building's also sends up a tall, slow smoke plume), then a soft scorch decal and rubble that sink away.
- No bloom: the game camera's colour pipeline stays as it is (AgX without HDR), so hot effects glow by additive colour, not post-processing.
- Cost: a 200-frame clip of an Inferno Artillery strike (up to 228 particles alive at once) renders at a median 19.7 ms a frame at 1280×720 on the build host's integrated GPU, no slower in its busiest tenth than in its quietest frames.

## Budget

The headless screenshot mode prints the mesh count and the average time of 30 rendered frames, each waited out on the GPU. That is serialized render time, not a windowed frame rate (no vsync or pipelining), and it is noisy: two identical 1080p runs of the densest original map (`zh:Red Rock`, about 35,000 meshes, mostly scenery) gave 9.2 and 13.3 ms on the build host's integrated GPU. The look pass roughly doubled the mesh count (2,800-3,000 to 5,500-7,700 on typical maps). Frame rate in a real window has not been measured.

## Gaps vs the alpha (October 2013 pass)

Measured and compared against frames of `[dtl2Q_tUDDQ]`, `[3tlbDYSUboc]` and `[6XKby6Qz5oI]`. Matched now: ground colour (within about 10%), centre brightness (117-125 against 125-142 sRGB luminance), contact shadows, tree silhouettes. Still open, largest first:

- Few ambient effects: no smoke from stacks or dust under vehicles (`view.rs`).
- No animation: rotors, treads, walking infantry, pumpjacks (`view.rs`, `models.rs`).
- Building walls read flat: the footage's painted panel texture and wear can't come from primitives alone; a procedural panel/grime texture per slot would (`view.rs` materials).
- EU roofs are nearly one tone; the footage has more orange trim and darker roof plates (`models.rs`).
- Artillery barrels can't pitch: parts have no pitch rotation (`models.rs` `Part`).
- Masts and dishes are too small to read at the default zoom (`models.rs`).
- The HUD is our plain panels, not the footage's framed style (`hud.rs`).
- Each acacia is 16 meshes; merging a crown's lumps into one prebuilt mesh would cut that to 4 if tree-dense maps prove slow (`terrain.rs`).
