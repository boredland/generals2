#!/usr/bin/env bash
# Runs a command against a virtual X display (Xvfb), for exercising the windowed client on a
# host without a display. Xvfb and xdotool come from the distro packages, unpacked under
# ~/.local/xvfb without root:
#   mkdir -p ~/.local/xvfb && cd ~/.local/xvfb
#   apt-get download xvfb xdotool libxdo3 x11-apps libxfont2 libxkbfile1
#   for d in *.deb; do dpkg-deb -x "$d" root; done
# The client must render through lavapipe there (the GPU's Vulkan driver can't present to a
# server without DRI3):
#   VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan
# Usage: [XVFB_SCREEN=WxH] scripts/xvfb.sh :DISPLAY COMMAND...   (starts the display if needed)
set -euo pipefail
root=$HOME/.local/xvfb/root
export LD_LIBRARY_PATH=$root/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
export PATH=$root/usr/bin:$PATH
display=$1
shift
if ! [ -e "/tmp/.X11-unix/X${display#:}" ]; then
  # Keyboard data from the host's xkb-data package. No GLX: the server's own GL acceleration
  # hangs in a GPU fence wait on this host, and the client renders through Vulkan anyway.
  setsid "$root/usr/bin/Xvfb" "$display" -screen 0 "${XVFB_SCREEN:-1600x900}x24" -nolisten tcp \
    -xkbdir /usr/share/X11/xkb -extension GLX </dev/null >"/tmp/xvfb${display#:}.log" 2>&1 &
  for _ in $(seq 50); do
    [ -e "/tmp/.X11-unix/X${display#:}" ] && break
    sleep 0.1
  done
fi
DISPLAY=$display exec "$@"
