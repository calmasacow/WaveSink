#!/usr/bin/env bash
set -euo pipefail

if [ "$(uname -m)" != x86_64 ] || ! command -v pacman >/dev/null; then
  echo "WaveSink Arch/Omarchy installer requires x86_64 and pacman" >&2
  exit 1
fi
for tool in curl sha256sum awk sudo; do command -v "$tool" >/dev/null || { echo "Missing $tool" >&2; exit 1; }; done

url=$(curl -fsSL -o /dev/null -w '%{url_effective}' https://github.com/calmasacow/WaveSink/releases/latest)
tag=${url##*/}
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Invalid stable release: $url" >&2; exit 1; }
version=${tag#v}
asset="wavesink-bin-${version}-1-x86_64.pkg.tar.zst"
base="https://github.com/calmasacow/WaveSink/releases/download/$tag"
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
curl -fsSL --retry 3 "$base/SHA256SUMS" -o "$dir/SHA256SUMS"
curl -fsSL --retry 3 "$base/$asset" -o "$dir/$asset"
expected=$(awk -v name="$asset" '$2 == name { print $1 }' "$dir/SHA256SUMS")
[[ $expected =~ ^[[:xdigit:]]{64}$ ]] && [ "$(sha256sum "$dir/$asset" | cut -d' ' -f1)" = "$expected" ] || {
  echo "WaveSink checksum missing or mismatched" >&2; exit 1;
}
sudo pacman -U -- "$dir/$asset"
