# Control socket

WaveSink listens on a local Unix socket so stream controllers and scripts can
drive the mixer and follow it live. The OpenDeck plugin uses it.

```text
$XDG_RUNTIME_DIR/wavesink/control.sock     (mode 0600, directory 0700)
```

`WAVESINK_CONTROL_SOCKET` overrides the path. Only the user running WaveSink
can connect.

## Wire format

Newline-delimited JSON, one object per line, at most 64 KiB per request.

```json
{"id": 1, "method": "adjust_mix_volume", "params": {"mix": "sink_bus_stream", "delta": -5}}
{"id": 1, "result": null}
```

A failure replies `{"id": 1, "error": "unknown mix sink_bus_nope"}`. `id` is
echoed as given, or `null` when it's missing. Requests on one connection run in order.

## Methods

| Method | Params | Notes |
|---|---|---|
| `hello` | | `{app, version, protocol}` |
| `get_state` | | the [snapshot](#snapshot) |
| `subscribe` | `state` (default true), `levels` (default false) | the first `state` event arrives before the reply |
| `set_input_volume` | `input`, `volume` 0-100 | |
| `adjust_input_volume` | `input`, `delta` | clamped to 0-100 |
| `set_input_mute` | `input`, `muted`? | omit `muted` to toggle |
| `set_mix_volume` | `mix`, `volume` | |
| `adjust_mix_volume` | `mix`, `delta` | |
| `set_mix_mute` | `mix`, `muted`? | omit `muted` to toggle |
| `set_route` | `input`, `mix`, `enabled`?, `send`?, `muted`? | omitted fields keep their value |
| `toggle_route` | `input`, `mix`, `field` `"enabled"` (default) or `"muted"` | |
| `adjust_route_send` | `input`, `mix`, `delta` | |
| `toggle_solo` | `input` | the same as right-clicking the input's icon |
| `load_profile` | `name` | |
| `cycle_profile` | `direction` 1 or -1 | wraps |
| `show_window` | | |

Every change goes through the same commands as the UI, so the 100% cap and
the routing rules apply. The UI updates to match.

## Events

```json
{"event": "state", "data": { ...snapshot... }}
{"event": "levels", "data": {"inputs": {"sink_game": 0.42}, "mixes": {"sink_bus_stream": 0.3}}}
```

`state` is pushed whenever anything visible in the snapshot changes,
whatever changed it (the UI, the tray, hotkeys, profile triggers). `levels`
are linear peaks (0-1, the louder channel), about 15 times a second while
any client wants them, even with the window in the tray.

## Snapshot

```json
{
  "protocol": 1,
  "inputs": [
    {"id": "sink_game", "label": "Game", "kind": "software",
     "icon": "game", "icon_color": "blue", "volume": 80, "muted": false,
     "routes": {"sink_bus_stream": {"enabled": true, "send": 100, "muted": false}}}
  ],
  "mixes": [
    {"id": "sink_bus_stream", "label": "Stream", "icon": null,
     "icon_color": null, "volume": 100, "muted": false}
  ],
  "solo": null,
  "profiles": ["Default", "Gaming"],
  "active_profile": "Default"
}
```

Inputs and mixes are in matrix order. Each input's `routes` lists every mix.

## Try it

```bash
socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/wavesink/control.sock
{"id":1,"method":"subscribe","params":{"levels":false}}
```
