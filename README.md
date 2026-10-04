# WaveSink
<p align="center">
  <img src="assets/WaveSinkLogo.png" alt="WaveSink" width="220">
</p>



Linux-native input/mix routing for PipeWire, inspired by the workflow of
modern creator mixers.

Group unlimited apps into software inputs such as Game, Chat, and Music;
then build independent Personal, Stream, Chat, and custom mixes for
monitoring and capture. Hardware inputs join the same matrix with independent
routing, levels, and visual identity.

![WaveSink mixer](assets/screenshot-2026-09-27_19-46-48.png)

```
 inputs ──► independent route cells ──► Personal / Stream / Chat mixes
                                               ├──► headphones / speakers
                                               └──► WaveSink: <mix> for OBS/Discord
```

## Features

- **Inputs and mixes** - software channels group unlimited apps under one
  source fader; every input×mix cell has its own on/off, send level, and
  mute, so each mix keeps its own balance without touching the others
- **Apps** - running apps appear automatically; assign once, remembered
  forever
- **Mixes** - each mix is a recordable source (`<name> (WaveSink)`): in OBS
  or Discord, add the mix as an audio input instead of Desktop Audio. A mix
  plays to any output devices you pick, or to the system default, and its
  slider is the one volume for all of them. New inputs and mixes start
  unrouted - tick the cells you want.
- **No amplification** - every level tops out at 100% (unchanged audio)
- **Equalizer** - per-input parametric EQ (up to 10 bands) with a
  draggable response curve, bundled community presets, and import/export
  including AutoEq text blocks
- **Hardware inputs** - add mics and capture devices to the matrix and route
  each independently to any mix, with optional Audio FX (noise gate,
  compressor, limiter) applied before every mix it feeds
- **Meters** - live levels inside every slider; optional Pro Audio Metering
  (dBFS scale, dB labels, peak readout) and adjustable meter frame rate
- **Profiles** - save and switch full layouts from the tray
- **Themes** - Original plus all 22 stock Omarchy palettes, available on every
  distro. Omarchy installs can also follow their live desktop palette.

![Equalizer](assets/EQ_ScreenShot.png)
![Apps](assets/App_selectScreenshot.png)

## Install

Each script fetches the newest stable release, verifies its SHA256SUMS entry,
and installs it. Packages are for x86_64. WaveSink does not publish to AUR or
COPR.

**Arch / Omarchy:**

```bash
curl -fsSLO https://raw.githubusercontent.com/calmasacow/WaveSink/main/scripts/install/arch.sh && bash arch.sh
```

**Ubuntu 24.04 / 26.04 LTS, Linux Mint 22, Pop!_OS 24.04:**

```bash
curl -fsSLO https://raw.githubusercontent.com/calmasacow/WaveSink/main/scripts/install/ubuntu.sh && bash ubuntu.sh
```

**Fedora 43 / 44:**

```bash
curl -fsSLO https://raw.githubusercontent.com/calmasacow/WaveSink/main/scripts/install/fedora.sh && bash fedora.sh
```

**NixOS (user profile):**

```bash
curl -fsSLO https://raw.githubusercontent.com/calmasacow/WaveSink/main/scripts/install/nixos.sh && bash nixos.sh
```

**Other distros — portable AppImage, no root:**

```bash
curl -fsSLO https://raw.githubusercontent.com/calmasacow/WaveSink/main/scripts/install/appimage.sh && bash appimage.sh
```

Run `~/.local/share/AppImages/WaveSink.AppImage` afterward. For a launcher entry,
use [Gear Lever](https://flathub.org/apps/it.mijorus.gearlever) or AppImageLauncher.

For manual downloads, see [Releases](https://github.com/calmasacow/WaveSink/releases).

Requires PipeWire with `pipewire-pulse` and WirePlumber 0.5+ (the default
on most current distros).

## Build

```bash
npm install
npm run tauri dev      # run
npm run tauri build    # package
```

Config lives in `~/.config/wavesink` as plain JSON. Existing `~/.config/sink`
state moves there automatically on first launch.

The routing contract is documented in [docs/routing-model.md](docs/routing-model.md).
On first launch after upgrading, old JSON files are backed up beside the new
`routing.json`; see the migration notes there for the old Sink mapping.

## Contact

For bugs, feature requests, or support, open a
[GitHub issue](https://github.com/calmasacow/WaveSink/issues).

## License

[GPL-3.0](LICENSE)

Bundled Omarchy color palettes are used under the MIT License; see
[third-party notices](THIRD_PARTY_LICENSES.md).

## Credits

- [Sink](https://github.com/NC1107/sink) by Nicholas Conn - original project
  and inherited codebase.
- [Omarchy](https://github.com/basecamp/omarchy) - bundled stock color palettes.
- [Material Symbols](https://fonts.google.com/icons) - interface icon font.
- [Fira Code](https://github.com/tonsky/FiraCode) - bundled application font.
- [PipeWire](https://pipewire.org/) - Linux audio graph foundation.
- [Tauri](https://tauri.app/) - desktop application framework.
