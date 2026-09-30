#!/usr/bin/env bash
set -euo pipefail

# shellcheck source=/dev/null
. /etc/os-release
case "${ID}:${VERSION_ID}" in
  ubuntu:24.04|ubuntu:26.04|linuxmint:22|linuxmint:22.*|pop:24.04) ;;
  *) echo "Supported: Ubuntu 24.04/26.04, Linux Mint 22, Pop!_OS 24.04 (x86_64)" >&2; exit 1 ;;
esac
[ "$(uname -m)" = x86_64 ] || { echo "WaveSink needs x86_64" >&2; exit 1; }
for tool in curl sha256sum awk sudo apt; do command -v "$tool" >/dev/null || { echo "Missing $tool" >&2; exit 1; }; done

url=$(curl -fsSL -o /dev/null -w '%{url_effective}' https://github.com/calmasacow/WaveSink/releases/latest)
tag=${url##*/}
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Invalid stable release: $url" >&2; exit 1; }
asset="wavesink_${tag#v}_amd64.deb"
base="https://github.com/calmasacow/WaveSink/releases/download/$tag"
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
curl -fsSL --retry 3 "$base/SHA256SUMS" -o "$dir/SHA256SUMS"
curl -fsSL --retry 3 "$base/$asset" -o "$dir/$asset"
expected=$(awk -v name="$asset" '$2 == name { print $1 }' "$dir/SHA256SUMS")
[[ $expected =~ ^[[:xdigit:]]{64}$ ]] && [ "$(sha256sum "$dir/$asset" | cut -d' ' -f1)" = "$expected" ] || {
  echo "WaveSink checksum missing or mismatched" >&2; exit 1;
}
sudo apt install -- "$dir/$asset"
