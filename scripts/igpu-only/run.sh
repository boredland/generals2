#!/usr/bin/env bash
# Runs a command on the integrated Radeon only. The MI50 (a Vega 20 card that hangs any process
# opening it) is hidden at the open() level, so no Vulkan/GL driver ever issues an ioctl to it.
# Usage: scripts/igpu-only/run.sh <command> [args...]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
lib="$here/build/hide_dri.so"
if [ ! -f "$lib" ] || [ "$here/hide_dri.c" -nt "$lib" ]; then
  mkdir -p "$here/build"
  cc -O2 -shared -fPIC -Wall -Wextra -o "$lib" "$here/hide_dri.c" -ldl
fi

# Find the MI50 by its PCI IDs, not its address: a reboot can renumber the bus (the iGPU moved
# from c8:00.0 to c5:00.0 once), and the card can drop off the bus entirely.
vega20=" 0x66a0 0x66a1 0x66a2 0x66a3 0x66a4 0x66a7 0x66af "
hide=()
keep=()
drm=${G2_SYS_DRM:-/sys/class/drm}  # overridable only to test this script against a fake tree
for n in "$drm"/card[0-9] "$drm"/card[0-9][0-9] "$drm"/renderD*; do
  [ -e "$n/device/vendor" ] || continue
  if [ "$(cat "$n/device/vendor")" = 0x1002 ] && [[ "$vega20" == *" $(cat "$n/device/device") "* ]]; then
    hide+=("$(basename "$n")")
  else
    keep+=("$(basename "$n")")
  fi
done
[[ " ${keep[*]-} " == *" renderD"* ]] || { echo "no render node left besides the MI50; refusing to run" >&2; exit 1; }

if [ ${#hide[@]} -gt 0 ]; then
  export G2_HIDE_DRI="$(IFS=:; echo "${hide[*]}")"
  export LD_PRELOAD="$lib${LD_PRELOAD:+:$LD_PRELOAD}"
else
  # No MI50 on the bus: nothing can reach it, so there is nothing to hide.
  echo "run.sh: no MI50 found; running without the shield" >&2
fi
export VK_DRIVER_FILES=/usr/share/vulkan/icd.d/radeon_icd.json
export WGPU_BACKEND=vulkan
exec "$@"
