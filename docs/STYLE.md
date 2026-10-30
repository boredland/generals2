# Visual style

Target mood: the 2013 alpha gameplay footage of the cancelled game: a warm, hard-lit military RTS seen from a steep angle, on dry savanna ground, with a spotlit centre. We match the **mood and readability**, not the content. Every model, texture and UI element is designed here and generated procedurally; no shapes, icons or layouts are traced from the footage. The one exception is the menus, which copy Command & Conquer: Generals' own (see Menus), by choice.

Reference footage (local-only):

- `archive/videos/alpha/…[dtl2Q_tUDDQ].mkv`: 1080p closed-alpha gameplay, 24 Aug 2013 (primary mood reference).
- `archive/videos/wiki/…[A9g_pat78eA].mkv`, `archive/videos/ia/tweakers008132/`, `archive/videos/sweep/…[LuHGrw0AEl8].mkv`: 2013 stage and live demos.

## Camera (`look.rs`, `camera.rs`)

- Steep three-quarter view, about 57° below the horizon, narrow 32° field of view. Buildings read almost isometric.
- Close default zoom: a command center spans about a fifth of the screen width. This is the Close battlefield camera, the default; the options page also offers OpenRA's Medium (1.4 times as much ground) and Far (2 times). The default view is the furthest out a player can zoom; the wheel zooms in to half of it. The haze and the sun's shadow range are tuned at Close and stretch with any view further out, so a wider view keeps the same look around its centre.
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

Proportions are the alpha's: footprints are its `EntityRadius` and build grids, imported models are drawn at its own scale (one alpha unit, one world unit), and procedural vehicles are 1.5 times longer than their footprint is wide, the alpha's median for its vehicles. A procedural building is 1.75 × its radius wide, inside its build grid as the alpha's are: the EU war factory 21 wide (the alpha's mesh 21.6), the Aegis 10.5 long (9.7).

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
- **Feedback** (`tooltip.rs`, `alerts.rs`, `feedback.rs`, the `input.rs` overlays): a grid button's tooltip in the same panel style, above the grid, title in teal and what blocks it in red; an under-attack line in red under the match bar, fading out over its last second, with a red ring closing in on the minimap and pulsing where it happened; order marks as thin gizmo rings on the ground (green move, orange attack-move, teal rally) that shrink and fade in half a second, or the target's ring flashing twice; a selected factory's rally point as a teal line out to a small flag. Thin lines and one accent colour each, so they read over the battle without covering it.
- Not shown until the systems exist: generals and perks, abilities, experience.

## Menus (`menu.rs`, `front.rs`)

The menus are laid out as Command & Conquer: Generals' shell, from its window layouts in EA's GPL release (`vendor/superhackers/GeneralsGamePatch2/Generals/Window/Menus/*.wnd`): positions in their 800×600 coordinates, colours from their draw data. Only the layout and colours are taken; Generals' art (its logo, button textures, the "Generals" font) is EA's and isn't used.

- Every screen is the layouts' 800×600 canvas, centred and scaled to the window's height. The battlefield behind it stands in for Generals' animated shell map; the ruled gold frame and the darker margin outside it are its `MainMenuRuler`.
- Colours from the layouts: gold edges and headings (255 190 1), rust list and cell borders (122 48 1), orange-red button labels (226 76 23) that turn amber under the cursor (255 191 0), and the panels' blacks (2 2 2 at 193 and 126 of 255). Text is set in DejaVu Sans Condensed (bundled, Bitstream Vera licence) in place of Generals' condensed faces.
- **Main** (`MainMenu.wnd`): the logo top right, where ours is the project's name, and the button column in its gold-edged box below it: Skirmish, Onslaught, Generals, 2013 Alpha, Options, Controls, Exit Game.
- **Skirmish** (`SkirmishGameOptionsMenu.wnd`): eight player rows with Player, Color, Faction and General cells (General where Generals has Team); a click steps a cell. An AI's Player cell steps Easy, Normal, Onslaught, then closes the last seat; Open seats exist up to the map's starts. Map Preview with the map's name and SELECT MAP on the right; Starting Cash, Fog of War and Seed under the rows; the player's general in the career box; the match in the info box; START GAME and BACK.
- **Map selection** (`SkirmishMapSelectMenu.wnd`): the map list with its red scroll bar, the picked map's preview beside it, ACCEPT and BACK; BACK restores the map from before.
- **In-game menu** (`QuitMenu.wnd`): the logo over Options, Surrender, Exit to Main Menu, Return to Game (Generals has Options there too, above them).
- **Options** (`OptionsMenu.wnd`): its parent window, title and rule, a bordered Control Options panel where Generals has its, and DEFAULTS, ACCEPT and BACK at its button positions. The settings in it are OpenRA's camera and input settings, not Generals' (which are display, audio and network options); each is a combo cell. Below the panel, OpenRA's description of what each mouse button does under the chosen scheme. Opened from the in-game menu it sits over the dimmed match, as that menu does.
- **Score screen** (`ScoreScreen.wnd`): gold column heads over a row of totals per player, the winner marked green, the outcome across the top band, the match in the box below, and the buttons along the bottom.
- **Generals**, **2013 Alpha** (importing the player's copy: FIND CNC.EXE opens the system's file dialog, the page follows the import) and **Controls** use the skirmish page's frame. With the player's import of the 2013 alpha, the Generals page shows its general portraits; without one, faction-coloured tiles.

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
