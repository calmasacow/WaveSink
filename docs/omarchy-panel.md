# Omarchy audio panel

WaveSink embeds the `wavesink.audio` widget in its executable. On first launch
on Omarchy, it installs into `$XDG_CONFIG_HOME/omarchy/plugins/wavesink.audio`
and replaces the existing audio widget in the user's `shell.json`. This applies
to native packages, AppImage and source builds; no separate installer download
or privileged desktop configuration is needed. Python 3 and Omarchy's current
Quickshell panel API are required. Other desktops are skipped. Automatic setup leaves layouts with no audio widget untouched; the explicit
**Enable WaveSink panel** button can add it back.

The panel lists every mix and input in current profile order. Select **Bar**
beside a mix to choose what scrolling/right-clicking the bar icon controls.
That choice is saved by stable mix ID, separately for each profile. Nothing is
selected by default. Deleted or unavailable targets never redirect controls
to a different mix. Mix/input changes, labels and profile switches follow the
same control socket used by OpenDeck. All levels remain within 0–100%.

Panel sliders accept dragging and keyboard adjustment. Mouse-wheel events
scroll the popup; they never change slider values. Each opening starts at the
top. Qt's normal Tab/Shift+Tab focus order, slider arrow keys and Escape work;
hovering rows does not move focus or scroll. Delegates keep their identities
across ordinary state updates, avoiding scroll jumps while dragging.

Mix meters read actual mix output. Scale and green/yellow/red color zones match
WaveSink's Standard or Pro Audio Metering setting, with colors read live from
the current Omarchy palette.
Input meters read the processed input where FX is enabled. The panel does not
bind Quickshell PipeWire objects or expose internal FX streams as user controls.
Available devices are queried while open; hardware controls and routing are
configured in the full mixer through **Open full mixer · hardware and routing**.
Every monitor gets its own popup and unique IPC target. Meter subscription is
enabled only while that popup is open. Socket reconnect retries handle WaveSink
starting, stopping and restarting without restarting the desktop or audio stack.

## Restore and uninstall

Choose **Restore OS panel** in Settings or **Restore OS audio panel** in the
popup. Both show an explanation, then a second confirmation with instructions
for enabling WaveSink again. Canceling either step keeps the panel installed.

Settings → Omarchy shows an **Active** or **Inactive** badge for the panel.
Click **Enable WaveSink panel** when inactive, or **Reapply panel** when active
to repair or refresh the integration. The button is disabled while applying,
and a status message confirms success or reports an error. Status refreshes
when the window regains focus. Enabling works even if panel files or its bar
entry were removed; no reinstall is needed. The profile selector and **Open full mixer** button share a fixed
footer below the scrolling controls.

The CLI can also restore it:

```sh
wavesink --omarchy-panel remove
```

Restore runs entirely as the desktop user and changes only WaveSink's widget
references. Unrelated current settings survive. A disabled integration stays
disabled across WaveSink restarts and upgrades. Enable it again with the Settings button or
with `wavesink --omarchy-panel install`. `wavesink --omarchy-panel status` reports
whether the panel is enabled. These commands never start the audio engine.

Restore the panel before removing WaveSink's package/AppImage. If the application
has already been removed, the popup's restore button still works: its Python
cleanup script lives with the user plugin. Restoring the widget does not stop
WaveSink or change audio routing. Before removing the mixer itself, select
physical system input/output defaults if they currently refer to WaveSink's
virtual devices.

Setup snapshots the prior shell config and existing plugin files under
`$XDG_CONFIG_HOME/wavesink/omarchy-panel-backups/`. Writes are atomic, concurrent
setup processes are locked, and a concurrently changed shell config aborts
activation. Existing user audio clones are discovered from their manifests;
they remain installed for restoration. Panel assets use content-specific paths so Omarchy reloads imported QML and JavaScript on upgrades without restarting the shell. No packaged Omarchy files are modified.

## Checks

```sh
python3 packaging/omarchy/check.py
node packaging/omarchy/check-model.cjs
qmllint packaging/omarchy/Panel.qml packaging/omarchy/LevelRow.qml
cargo test --lib
```

These cover fresh/repeated installs, upgrades, nested bar layouts, custom clones,
moved widgets, opt-out, restore preserving later changes, malformed config,
dynamic row identity and the meter scale. A live Omarchy session is also needed
to check popup placement, scrolling, keyboard focus and multiple monitors.
