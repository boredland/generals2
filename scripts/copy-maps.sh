#!/usr/bin/env bash
# Copies the official Generals / Zero Hour maps a release package carries into DST, in the
# layout g2_maps::catalogue expects (<Game>/Maps/official/<Name>/<Name>.map plus the .tga
# preview the skirmish page shows), with EA's licence for them.
#
# Usage: scripts/copy-maps.sh DST
set -euo pipefail
cd "$(dirname "$0")/.."

DST=$1
SRC=vendor/ea/CnC_Modding_Support
[ -d "$SRC/Zero Hour/Maps/official" ] || { echo "map sources missing: git submodule update --init $SRC" >&2; exit 1; }
for game in Generals "Zero Hour"; do
    (cd "$SRC/$game/Maps/official" && find . \( -name '*.map' -o -name '*.tga' \) -print0) |
        while IFS= read -r -d '' f; do
            mkdir -p "$DST/$game/Maps/official/$(dirname "$f")"
            cp "$SRC/$game/Maps/official/$f" "$DST/$game/Maps/official/$f"
        done
done
# EA's GPL-3.0 release with its section 7 additional terms must travel with the data.
cp "$SRC/LICENSE.md" "$DST/LICENSE-EA.md"
