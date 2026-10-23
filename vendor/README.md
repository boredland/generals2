# vendor/

Upstream source vendored as git submodules. Nothing in `vendor/` is modified in place. Our own code, scripts and patches live outside `vendor/`; patches needed to build upstream go in `patches/<repo>/` and are applied to a scratch worktree under `build/` (see below).

Clone with `git clone --recursive`, or run `git submodule update --init --recursive`. `CnC_Modding_Support` is a shallow (`--depth 1`) submodule.

## Pins

Pinned 2026-10-23. Each pin is the default-branch HEAD at add time, except GeneralsGameCode, which is checked out at its newest `weekly-*` release tag.

| Path | Upstream | Ref | SHA | License | Purpose |
|---|---|---|---|---|---|
| `ea/CnC_Generals_Zero_Hour` | [electronicarts/CnC_Generals_Zero_Hour](https://github.com/electronicarts/CnC_Generals_Zero_Hour) | `main` | `0a05454d` | GPL-3.0 + EA §7 terms | EA's original 2025 source release of Generals / Zero Hour (SAGE engine) |
| `ea/CnC_Modding_Support` | [electronicarts/CnC_Modding_Support](https://github.com/electronicarts/CnC_Modding_Support) | `main` (shallow) | `3aad8e41` | GPL-3.0 + EA §7 terms | EA modding sources: INI/XML/XSD definitions, shaders, map sources (no art) |
| `superhackers/GeneralsGameCode` | [TheSuperHackers/GeneralsGameCode](https://github.com/TheSuperHackers/GeneralsGameCode) | tag `weekly-2026-10-16` | `63c78982` | GPL-3.0 + EA §7 terms | The actively maintained modernization of the EA code (CMake, VC6/MSVC2022, bug fixes). Nested submodule `GeneralsReplays` @ `0711bf9d` |
| `superhackers/GeneralsDocuments` | [TheSuperHackers/GeneralsDocuments](https://github.com/TheSuperHackers/GeneralsDocuments) | `main` | `511719d0` | none stated | Format and tooling notes (`documents/{w3d,big,csf,dds,ini,wnd,scb,worldbuilder,…}`), addon-number registry, Generals/ZH artwork |
| `superhackers/GeneralsWiki` | [TheSuperHackers/GeneralsWiki](https://github.com/TheSuperHackers/GeneralsWiki) | `main` | `dbeaedb6` | none stated | Technical wiki: `SourceCode/` (engine internals) and `Asset/` (Art, Audio, GUI, GameDesign, Localization, Maps, Tools) |
| `superhackers/GeneralsBigCreator` | [TheSuperHackers/GeneralsBigCreator](https://github.com/TheSuperHackers/GeneralsBigCreator) | `main` | `c7e43fac` | MIT | `.big` archive creator |
| `superhackers/bink-sdk-stub` | [TheSuperHackers/bink-sdk-stub](https://github.com/TheSuperHackers/bink-sdk-stub) | `master` | `f661603a` | GPL-3.0 | Stub for the proprietary Bink video SDK |
| `superhackers/miles-sdk-stub` | [TheSuperHackers/miles-sdk-stub](https://github.com/TheSuperHackers/miles-sdk-stub) | `master` | `6e32700d` | GPL-3.0 | Stub for the proprietary Miles audio SDK |
| `superhackers/min-dx8-sdk` | [TheSuperHackers/min-dx8-sdk](https://github.com/TheSuperHackers/min-dx8-sdk) | `master` | `7bddff8c` | none stated (Microsoft SDK excerpt) | Minimal DirectX 8 headers/libs needed to build |
| `superhackers/stlport-4.5.3` | [TheSuperHackers/stlport-4.5.3](https://github.com/TheSuperHackers/stlport-4.5.3) | `master` | `50182406` | STLport license (in sources) | STLport patched as in the EA release (VC6 builds) |
| `superhackers/zlib-1.1.4` | [TheSuperHackers/zlib-1.1.4](https://github.com/TheSuperHackers/zlib-1.1.4) | `main` | `ac753eee` | zlib | zlib version the engine shipped with |
| `superhackers/lzhl-1.0` | [TheSuperHackers/lzhl-1.0](https://github.com/TheSuperHackers/lzhl-1.0) | `main` | `e5d4a830` | none stated | LZH-Light compression used by the engine |
| `ports/Fighter19-linux` | [Fighter19/CnC_Generals_Zero_Hour](https://github.com/Fighter19/CnC_Generals_Zero_Hour) | `develop` | `088f3e61` | GPL-3.0 + EA §7 terms | Native Linux port (DXVK/SDL). Other branches, recorded but not checked out: `sdl-gpu`, `gles`, `emscripten`, `networking`, `networking-new` |
| `ports/GeneralsX` | [fbraz3/GeneralsX](https://github.com/fbraz3/GeneralsX) | `main` | `afb09f14` | GPL-3.0 + EA §7 terms | Cross-platform (Linux/macOS) port |
| `reimpl/OpenSAGE` | [OpenSAGE/OpenSAGE](https://github.com/OpenSAGE/OpenSAGE) | `master` | `588ac477` | GPL-3.0, and because it ports EA code, also the EA §7 terms (`LICENSE.md` + `LICENSE-EA.md`) | C# clean-room reimplementation of SAGE, now partly ported from EA code |
| `reimpl/OpenSAGE-Docs` | [OpenSAGE/Docs](https://github.com/OpenSAGE/Docs) | `master` | `a41f431d` | none stated | SAGE file-format and systems documentation (`file-formats/`, `behaviors/`, `systems/`) |
| `reimpl/Thyme` | [TheAssemblyArmada/Thyme](https://github.com/TheAssemblyArmada/Thyme) | `develop` | `ccef1e11` | GPL-2.0-or-later | Reverse-engineered Zero Hour reimplementation (predates the EA release) |
| `tools/OpenSAGE.BlenderPlugin` | [OpenSAGE/OpenSAGE.BlenderPlugin](https://github.com/OpenSAGE/OpenSAGE.BlenderPlugin) | `master` | `53d77d38` | LGPL-3.0 | Blender W3D import/export. Nested submodule `blender_addon_updater` @ `981aa298` |
| `tools/pySAGE` | [ClementJ18/pySAGE](https://github.com/ClementJ18/pySAGE) | `master` | `4b0cdd84` | MIT | Python parsers for SAGE formats (`.map`, `.str`, …) |
| `tools/pyBIG` | [ClementJ18/pyBIG](https://github.com/ClementJ18/pyBIG) | `master` | `081f57f5` | MIT | Python `.big` archive reader/writer |
| `tools/cnc-formats` | [iron-curtain-engine/cnc-formats](https://github.com/iron-curtain-engine/cnc-formats) | `main` | `77da596e` | MIT OR Apache-2.0 | Rust parsers for C&C file formats |

Excluded because they are retail data overlays, not engine code: `TheSuperHackers/GeneralsGamePatch` (847 MB), `GeneralsControlBar` (718 MB), `GeneralsRussianLoca`, `GeneralsGermanUncutLoca`.

## Licensing split

- **EA-derived code** (`ea/*`, `superhackers/GeneralsGameCode`, `ports/*`, `reimpl/OpenSAGE`) is GPL-3.0 with EA's §7 additional terms: no use of EA trademarks, no claims of EA affiliation or endorsement, and modified versions must be marked as changed. Anything we derive from it inherits all of these terms.
- **Thyme** is GPL-2.0-or-later, so it can be used under GPL-3.0. `LICENSE` is the GPL-2.0 text, and 1256 of 1261 `src/` files carry "version 2 of the License, or (at your option) any later version"; the other 5 have no header.
- **BlenderPlugin** is LGPL-3.0. **pySAGE, pyBIG, GeneralsModBuilder, GeneralsBigCreator** are MIT. **cnc-formats** is MIT OR Apache-2.0.
- **min-dx8-sdk, lzhl, GeneralsDocuments, GeneralsWiki, OpenSAGE-Docs** have no license file. Use them as reference only.

## Building GeneralsGameCode on Linux

Verified 2026-10-23 on this host (Linux 7.0, rootless Docker):

```
scripts/build-generalsgamecode.sh          # all targets; optional args = ninja targets, e.g. z_generals
```

This produces Windows PE32 binaries (VC6 under Wine in Docker, preset `vc6`) in `build/GeneralsGameCode/build/docker/{Generals,GeneralsMD}/`. Running them needs a retail game install (not present here; `docker-install.sh` was intentionally not run).

The wrapper does three things beyond upstream `scripts/docker-build.sh`:

1. **Builds in a scratch worktree** (`build/GeneralsGameCode`, gitignored) at the submodule's pinned commit. The submodule stays pristine, and upstream's `build/docker/` output never lands inside `vendor/`.
2. **Applies `patches/GeneralsGameCode/*.patch`.** `0001-pathfinder-include-PreRTS.patch` adds `#include "PreRTS.h"` to the nine `Core/GameEngine/Source/GameLogic/AI/Pathfinder/*.cpp` files that were split out of `AIPathfind.cpp` in upstream PR #3429, first released in `weekly-2026-10-09`. Those files rely on `PreRTS.h` being force-included as the precompiled header. Upstream's Docker entrypoint passes `-DCMAKE_DISABLE_PRECOMPILE_HEADERS=1`, so without the patch the Docker build fails with `PathfindCellInfo.h(33) : error C2143` (and similar errors in PathfindCellList, PathfindZoneManager, PathNode and ZoneBlock). Turning PCH back on is not an option: under Wine, CMake's VC6 PCH flags break (`Command line error D2004 : '/FI' requires an argument`). The bug is still present on upstream `main` as of 2026-10-23 and should be reported upstream.
3. **Works under rootless Docker.** Upstream runs the container as `-u $(id -u):$(id -g)`. Under rootless Docker that uid maps to a host subuid, which cannot write the bind mount (`CMake Error: Unable to (re)create the private pkgRedirects directory`). Container root maps to the invoking user, so the wrapper builds the image with `UID=0 GID=0` and runs it as `0:0`. Under rootful Docker it uses the caller's uid as upstream does.

Result: `[2388/2388] Linking CXX executable Generals\WorldBuilderV.exe`, exit 0, 0 failed steps.

```
6041665  Generals/generalsv.exe         PE32 executable (GUI) Intel 80386, for MS Windows
3809343  Generals/guiedit.exe
  94275  Generals/imagepacker.exe
4571207  Generals/mapcachebuilder.exe
1630272  Generals/W3DViewV.exe
5992517  Generals/WorldBuilderV.exe
6373444  GeneralsMD/generalszh.exe      PE32 executable (GUI) Intel 80386, for MS Windows
3919937  GeneralsMD/guiedit.exe
  94277  GeneralsMD/imagepacker.exe
4862025  GeneralsMD/mapcachebuilder.exe
1654851  GeneralsMD/W3DViewZH.exe
 143423  GeneralsMD/wdump.exe
6348872  GeneralsMD/WorldBuilderZH.exe
```

To update the pin:

```
git -C vendor/superhackers/GeneralsGameCode fetch --tags
git -C vendor/superhackers/GeneralsGameCode checkout weekly-YYYY-MM-DD
git add vendor/superhackers/GeneralsGameCode
```

Then rerun the wrapper. Drop the patch once upstream fixes the bug; `git apply` fails loudly if the patch no longer applies.
