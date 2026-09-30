#!/usr/bin/env bash
set -euo pipefail

[ "$(uname -m)" = x86_64 ] || { echo "WaveSink needs x86_64" >&2; exit 1; }
for tool in curl sha256sum awk install; do command -v "$tool" >/dev/null || { echo "Missing $tool" >&2; exit 1; }; done
url=$(curl -fsSL -o /dev/null -w '%{url_effective}' https://github.com/calmasacow/WaveSink/releases/latest)
tag=${url##*/}
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Invalid stable release: $url" >&2; exit 1; }
asset="wavesink_${tag#v}_amd64.AppImage"
base="https://github.com/calmasacow/WaveSink/releases/download/$tag"
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
curl -fsSL --retry 3 "$base/SHA256SUMS" -o "$dir/SHA256SUMS"
curl -fsSL --retry 3 "$base/$asset" -o "$dir/$asset"
expected=$(awk -v name="$asset" '$2 == name { print $1 }' "$dir/SHA256SUMS")
[[ $expected =~ ^[[:xdigit:]]{64}$ ]] && [ "$(sha256sum "$dir/$asset" | cut -d' ' -f1)" = "$expected" ] || {
  echo "WaveSink checksum missing or mismatched" >&2; exit 1;
}
install -Dm755 "$dir/$asset" "${XDG_DATA_HOME:-$HOME/.local/share}/AppImages/WaveSink.AppImage"
echo "Installed at ${XDG_DATA_HOME:-$HOME/.local/share}/AppImages/WaveSink.AppImage"
