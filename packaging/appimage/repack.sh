#!/usr/bin/env bash
# Assemble the AppImage we actually ship. Both the dev and release workflows
# run this, so CI tests the same artifact users download rather than the raw
# `tauri build` output.
#
# Two things happen here that tauri cannot do for us:
#
#  - PipeWire is taken from the system. A bundled libpipewire has its SPA
#    plugin path baked in at build time and is wrong on every other distro,
#    so the native backend fails there. tauri-cli 2.12 stopped bundling it,
#    but older and future versions may not, so strip it either way.
#  - Because of that, a host without PipeWire kills wavesink in the dynamic
#    loader before main(), so its own error dialog never runs and the app
#    just vanishes. The preflight explains it instead.
set -euo pipefail

app_path=${1:?usage: repack.sh <path to wavesink_*.AppImage>}
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
app_dir=$(cd "$(dirname "$app_path")" && pwd)
app=$(basename "$app_path")

cd "$app_dir"
rm -rf squashfs-root
"./$app" --appimage-extract > /dev/null

rm -fv squashfs-root/usr/lib/libpipewire-0.3.so*

# Our AppRun is linuxdeploy's plus the preflight call; linuxdeploy's sources
# one hardcoded hook, so a new hook file would never be read. Fail if what it
# generated has changed rather than shipping a stale copy of it.
echo "eb0b254ac0dae6543e6dd7cd02e1baf40a060de95b927313122d6b46323c00aa  squashfs-root/AppRun" \
  | sha256sum -c -

# Keep the flags as a list: they must reach gcc as separate arguments.
mapfile -t gtk_flags < <(pkg-config --cflags --libs gtk+-3.0 | tr ' ' '\n' | grep .)
gcc -O2 -Wall -Wextra -Werror \
  -o squashfs-root/usr/bin/wavesink-pipewire-check \
  "$here/pipewire-check.c" "${gtk_flags[@]}"

# The preflight must not link libpipewire, or it would die exactly as the app
# does and explain nothing.
if ldd squashfs-root/usr/bin/wavesink-pipewire-check | grep -q pipewire; then
  echo "preflight links libpipewire; it must dlopen it instead" >&2
  exit 1
fi
install -m 755 "$here/AppRun" squashfs-root/AppRun

# --appimage-extract leaves every directory 0700 and AppRun.wrapped 0770.
# mksquashfs -root-owned would bake that in, and then no ordinary user could
# traverse the payload or exec the wrapper.
chmod -R u=rwX,go=rX squashfs-root

offset=$("./$app" --appimage-offset)
head -c "$offset" "$app" > runtime
mksquashfs squashfs-root payload.squashfs -root-owned -noappend -quiet -comp zstd
cat runtime payload.squashfs > "$app"
chmod +x "$app"
rm -f runtime payload.squashfs

# Everything the runtime mounts must stay readable, and every directory
# traversable, for a user who is not root.
test -x squashfs-root/AppRun
test -x squashfs-root/AppRun.wrapped
test -x squashfs-root/usr/bin/wavesink-pipewire-check
[ -z "$(find squashfs-root ! -type l ! -perm -o+r -print -quit)" ]
[ -z "$(find squashfs-root -type d ! -perm -o+x -print -quit)" ]

echo "repacked $app"
