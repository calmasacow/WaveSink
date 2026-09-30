#!/usr/bin/env bash
set -euo pipefail

# shellcheck source=/dev/null
. /etc/os-release
[ "$ID" = nixos ] && [ "$(uname -m)" = x86_64 ] || { echo "Supported: NixOS x86_64" >&2; exit 1; }
for tool in curl sha256sum awk nix; do command -v "$tool" >/dev/null || { echo "Missing $tool" >&2; exit 1; }; done

url=$(curl -fsSL -o /dev/null -w '%{url_effective}' https://github.com/calmasacow/WaveSink/releases/latest)
tag=${url##*/}
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Invalid stable release: $url" >&2; exit 1; }
base="https://github.com/calmasacow/WaveSink/releases/download/$tag"
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
curl -fsSL --retry 3 "$base/SHA256SUMS" -o "$dir/SHA256SUMS"
curl -fsSL --retry 3 "$base/flake.nix" -o "$dir/flake.nix"
expected=$(awk '$2 == "flake.nix" { print $1 }' "$dir/SHA256SUMS")
[[ $expected =~ ^[[:xdigit:]]{64}$ ]] && [ "$(sha256sum "$dir/flake.nix" | cut -d' ' -f1)" = "$expected" ] || {
  echo "WaveSink flake checksum missing or mismatched" >&2; exit 1;
}
nix --extra-experimental-features 'nix-command flakes' profile install "path:$dir"
