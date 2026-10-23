#!/usr/bin/env bash
# Builds GeneralsGameCode (Windows PE via Wine + VC6 in Docker) without touching vendor/.
# Differences from upstream scripts/docker-build.sh:
# - Builds from a detached git worktree at build/GeneralsGameCode, reset to the submodule's pinned commit,
#   with patches/GeneralsGameCode/*.patch applied; vendor/ stays pristine.
# - Rootless Docker maps container uid 1000 to a host subuid that cannot write the bind mount; container
#   root maps to the invoking user, so the image is built for and run as 0:0 there.
# Usage: scripts/build-generalsgamecode.sh [ninja targets...]   (default: everything)
set -euo pipefail
cd "$(dirname "$0")/.."
VENDOR="$PWD/vendor/superhackers/GeneralsGameCode"
SRC="$PWD/build/GeneralsGameCode"

commit=$(git -C "$VENDOR" rev-parse HEAD)
[ -e "$SRC/.git" ] || git -C "$VENDOR" worktree add --detach "$SRC" "$commit"
if [ "$(git -C "$SRC" rev-parse HEAD)" != "$commit" ]; then
  rm -rf "$SRC/build/docker"
fi
git -C "$SRC" checkout -q --detach -f "$commit"
git -C "$SRC" submodule update --init --recursive -q
for p in patches/GeneralsGameCode/*.patch; do
  [ -e "$p" ] && git -C "$SRC" apply "$PWD/$p"
done

if docker info --format '{{.SecurityOptions}}' | grep -q rootless; then
  uid=0 gid=0
else
  uid=$(id -u) gid=$(id -g)
fi

docker build --build-arg UID="$uid" --build-arg GID="$gid" "$VENDOR/resources/dockerbuild" -t zerohour-build
docker run --rm -u "$uid:$gid" -e MAKE_TARGET="$*" -e FORCE_CMAKE=false -v "$SRC:/build/cnc" zerohour-build
find "$SRC/build/docker" -maxdepth 2 -name '*.exe' -printf '%s\t%P\n' | sort -k2
