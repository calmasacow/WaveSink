# Elgato hardware controls

Open a hardware input's edit panel and select its physical capture source.
WaveSink shows controls for the exact USB unit attached to that source.
Changes apply immediately to hardware, outside the routing profile. Use
**Refresh** after adjusting the device in another app or with a physical dial.
OpenWave is not required, installed, or launched.

| USB VID:PID | Device | Controls |
| --- | --- | --- |
| `0fd9:007d` | Wave XLR | Gain, mute, headphones, 48 V, low impedance |
| `0fd9:00a6` | XLR Dock | Gain, mute, headphones, 48 V, low impedance |
| `0fd9:0070` | Wave:3 | Gain, mute, headphones, microphone/PC monitor mix |
| `0fd9:00c7` | XLR Dock MK.2 | Gain, mute, headphones, 48 V, low impedance, monitor mix |

Other IDs, including Wave:1 and unverified variants, have no vendor-control
profile. Ordinary PipeWire routing still works for devices Linux supports.
Product names alone never authorize USB access. A new model needs documented
USB identity, block lengths, fields and verification before adding it here.

The protocol follows [OpenWave's reference](https://github.com/rikkichy/openwave/blob/main/docs/protocol.md).
The `00c7` protocol has different transfers and block layouts; it is never
treated as a legacy Dock. MK.2 bank detection requires complete settings,
headphone and monitor reads. Stalls/short blocks permit the other documented
bank; disconnects and timeouts do not. A transient MK.2 read I/O error is retried
once on the same bank. Writes are never automatically retried.

## Phantom power

**Check microphone and cable compatibility, and reduce monitoring levels before
changing 48 V.** WaveSink requires confirmation for each manual change. Phantom
power is unavailable on Wave:3 and is never saved or restored by WaveSink
profiles. Bare Shure MV7X is dynamic and does not need phantom power.

## USB permissions

The deb/rpm packages include `70-wavesink-elgato.rules`; Arch packages repack
the same payload. The rules grant access to the active local desktop session,
only for the four documented IDs. They do not change audio drivers or grant
world-writable USB access.

Portable AppImage and source builds need the rule installed separately. From
the source checkout:

```sh
sudo install -Dm644 packaging/linux/70-wavesink-elgato.rules /etc/udev/rules.d/70-wavesink-elgato.rules
sudo udevadm control --reload-rules
```

Reconnect only the Elgato device afterward, then refresh its controls. NixOS
users can include the rule text with `services.udev.extraRules`. The app itself
never elevates privileges or installs system rules.

Quit other vendor-control clients before using this panel. WaveSink serializes
its operations and claims only vendor interface 3. It never detaches
`snd-usb-audio`, claims an audio interface, resets USB, changes alternate
settings or power-cycles ports. Noncooperating clients that issue raw endpoint-0
transfers without claiming the interface can still interfere.

Identity comes from the selected live PipeWire node's ALSA card and that card's
sysfs USB ancestor, bus/address, serial and sysfs device-directory incarnation. Stale
connection tokens are rejected. Every change reads current config, patches only
the documented field, writes only changed blocks, and reads actual state back.
Other DSP flags and reserved bytes are preserved.

## Legacy Dock capture startup

On `0fd9:00a6`, direct ALSA tests reproduced digital silence when playback
started before capture. Capture-first worked with both endpoints active. This
reproduction does not establish whether firmware or Linux's USB audio driver
needs the underlying fix.

When WaveSink uses this Dock as an input or headphone output, it keeps a capture
stream active and waits for its first PCM buffer before connecting its own
headphone mix links. The pin survives hiding the window and pausing meters.
Removing all WaveSink uses releases it; unplugging drops it, and replugging
rebuilds it. No audio is saved. A failed pin or a two-second timeout allows
playback and logs the problem rather than leaving headphones blocked.

This policy controls WaveSink's links. It cannot guarantee capture-first if
another application has already opened playback, and it does not repair a
device already streaming zeros. Boot/replug recovery with other playback
clients still needs an ALSA/firmware or session-manager solution. The startup
quirk is not applied to other Elgato models without equivalent physical tests.

## Validation

Protocol fixtures cover capabilities, ranges, reserved bytes, MK.2 bank fallback
and timeouts. UI tests cover phantom confirmation, stale device selections and
write errors. Native graph acceptance uses a private PipeWire server and
WirePlumber's policy-only profile, with no hardware monitors:

```sh
cargo test --lib
npm test
python scripts/test-elgato-startup.py
```

Physical acceptance performed on `00a6`: read-only controls correctly reported
gain, mute, phantom and headphones while live mic/Voice Only capture continued.
A gain write/restore passed at 49.5 → 49.0 → 49.5 dB, with mute, phantom,
headphones and other controls unchanged. Phantom switching was not exercised;
other models use protocol fixtures, not local physical certification. The graph policy still needs a physical boot/replug test
before claiming durable hardware recovery.
