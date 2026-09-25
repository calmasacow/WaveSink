# Sink

Linux-native input/mix routing for PipeWire, inspired by the workflow of
modern creator mixers.

Group unlimited apps into software inputs such as Game, Chat, and Music;
then build independent Personal, Stream, Chat, and custom mixes for
monitoring and capture. The processed microphone is another matrix input.

![Mixer](docs/mixer.png)

```
 inputs ──► independent route cells ──► Personal / Stream / Chat mixes
                                               ├──► headphones / speakers
                                               └──► Sink: <mix> for OBS/Discord
```

## Features

- **Inputs and mixes** - software channels group unlimited apps under one
  source fader; mixes are independent destinations with per-cell send,
  mute, and master controls
- **Apps** - running apps appear automatically; assign once, remembered
  forever
- **Mixes** - recordable sources for OBS. Master Mix carries everything;
  custom mixes can carry "everything except music" and stay current as
  channels change. A mix can also carry your processed microphone, so one
  input device holds your voice plus app audio (Sonar-style Stream Mix),
  and each member gets its own send level and mute inside the mix - what
  recorders hear, independent of your own volume (pop out a mix's levels
  from its strip). In OBS, add a mix as an audio input - not Desktop Audio. A mix can also
  sit with the output devices instead, if you would rather keep your
  recording list short; it stays capturable through its monitor.
- **Independent destinations** - Personal, Stream, Chat, and custom mixes
  keep separate balances. Monitor selection changes what you hear without
  rewriting app assignments; every mix is capturable as `Sink: <name>`.
- **Equalizer** - per-input parametric EQ (up to 10 bands) with a
  draggable response curve, bundled community presets, and import/export
  including AutoEq text blocks
- **Microphone** - noise gate, compressor and limiter into a virtual mic
  you select in Discord or OBS. Pairs well with
  [NoiseTorch](https://github.com/noisetorch/NoiseTorch) on the input for
  noise suppression before the chain.
- **Profiles** - save and switch full layouts from the tray
- **Themes** - Original, Tokyo Night, or Gruvbox Dark, to match
  the rest of your desktop

![Equalizer](docs/eq.png)
![Mic](docs/mic.png)
![Apps](docs/apps.png)

## Install

**Arch / Manjaro / EndeavourOS** - from the [AUR](https://aur.archlinux.org/packages/sink-bin):

```bash
yay -S sink-bin      # or: paru -S sink-bin
```

**Fedora** - from [COPR](https://copr.fedorainfracloud.org/coprs/nc1107/sink/):

```bash
sudo dnf copr enable nc1107/sink
sudo dnf install sink
```

Both track new releases, so you update through your package manager like any
other package.

Otherwise, grab the latest from [Releases](https://github.com/NC1107/sink/releases)
and install the file directly:

**Fedora / openSUSE**

```bash
sudo dnf install ./sink-*.x86_64.rpm
```

**Debian / Ubuntu / Mint**

```bash
sudo apt install ./sink_*_amd64.deb
```

**Arch / Manjaro / EndeavourOS**

```bash
sudo pacman -U ./sink-bin-*-x86_64.pkg.tar.zst
```

These install the app properly - launcher entry, icon, uninstall
through your package manager.

**Any other distro - AppImage (portable, no root)**

```bash
chmod +x sink_*_amd64.AppImage
./sink_*_amd64.AppImage
```

To get a launcher entry for an AppImage, use
[Gear Lever](https://flathub.org/apps/it.mijorus.gearlever) or
AppImageLauncher.

Requires PipeWire with `pipewire-pulse` and WirePlumber 0.5+ (the default
on most current distros).

## Build

```bash
npm install
npm run tauri dev      # run
npm run tauri build    # package
```

Config lives in `~/.config/sink` as plain JSON.

The routing contract is documented in [docs/routing-model.md](docs/routing-model.md).
On first launch after upgrading, old JSON files are backed up beside the new
`routing.json`; see the migration notes there for the Sonar/old-sink mapping.

For the current implementation status and an OpenCode-ready continuation plan,
see [OPENCODE_HANDOFF.md](OPENCODE_HANDOFF.md).

## Contact

If you need help or run into something broken, the discord is the fastest
way to reach me.

[![Discord](https://img.shields.io/badge/Discord-5865F2?logo=discord&logoColor=white)](https://discord.gg/jUMuSxGf6q)

## License

[GPL-3.0](LICENSE)
