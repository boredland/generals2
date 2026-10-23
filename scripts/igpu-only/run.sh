#!/usr/bin/env bash
# Runs a command on the integrated Radeon (c8:00.0) only. The MI50 (c7:00.0) is hidden at the
# open() level, so no Vulkan/GL driver ever issues an ioctl to it.
# Usage: scripts/igpu-only/run.sh <command> [args...]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
lib="$here/build/hide_dri.so"
if [ ! -f "$lib" ] || [ "$here/hide_dri.c" -nt "$lib" ]; then
  mkdir -p "$here/build"
  cc -O2 -shared -fPIC -Wall -Wextra -o "$lib" "$here/hide_dri.c" -ldl
fi

# Resolve node names from PCI addresses so a renumbering after reboot can't flip the choice.
hide_pci=0000:c7:00.0
use_pci=0000:c8:00.0
hide=()
for n in /sys/class/drm/card[0-9] /sys/class/drm/card[0-9][0-9] /sys/class/drm/renderD*; do
  [ -e "$n/device" ] || continue
  [ "$(basename "$(readlink -f "$n/device")")" = "$hide_pci" ] && hide+=("$(basename "$n")")
done
# An empty list would hide nothing and let the driver reach the wedged card again.
[ ${#hide[@]} -gt 0 ] || { echo "no DRM nodes found for $hide_pci; refusing to run unshielded" >&2; exit 1; }
[ "$(basename "$(readlink -f /sys/class/drm/renderD128/device)")" = "$use_pci" ] \
  || { echo "renderD128 is not $use_pci; refusing to guess" >&2; exit 1; }

export G2_HIDE_DRI="$(IFS=:; echo "${hide[*]}")"
export LD_PRELOAD="$lib${LD_PRELOAD:+:$LD_PRELOAD}"
export VK_DRIVER_FILES=/usr/share/vulkan/icd.d/radeon_icd.json
export WGPU_BACKEND=vulkan
exec "$@"
