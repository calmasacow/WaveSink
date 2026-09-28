# Arch packaging

`PKGBUILD` is the Arch package for WaveSink, `wavesink-bin`.
It repackages the release `.deb` (no source build), so it installs with system PipeWire and webkit2gtk and no toolchain.

The `archpkg` job in `.github/workflows/release.yml` stamps it with each release's version and `.deb` checksum, builds it with `makepkg`, and attaches the `.pkg.tar.zst` to the release.

Install: download the `.pkg.tar.zst` from a [WaveSink release](https://github.com/calmasacow/WaveSink/releases), then run `sudo pacman -U ./wavesink-bin-*-x86_64.pkg.tar.zst`.
