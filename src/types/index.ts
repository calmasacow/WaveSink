// Mirrors the Rust structs in src-tauri/src/audio/types.rs - keep in sync.

export interface AppStream {
  index: number;
  /** Unique, never-reused stream id (PipeWire's object.serial). */
  serial: number;
  app_name: string;
  match_prop: string;
  /** Raw property value (stream identity for persistence). */
  match_value: string;
  /** User-chosen display name overriding app_name. */
  alias: string | null;
  icon_name: string | null;
  /** Resolved absolute icon file path (desktop-entry based). */
  icon_path: string | null;
  /** Producing process id (used backend-side for icon resolution). */
  pid: number | null;
  /** Name of the virtual sink the stream is routed to, if any. */
  assigned_sink: string | null;
  volume_percent: number;
  muted: boolean;
  /** True while the stream is actively producing audio. */
  active: boolean;
}

export interface VirtualSink {
  /** e.g. "sink_game" */
  name: string;
  /** e.g. "Game" */
  label: string;
  /** Material Symbol for the strip icon. */
  icon: string | null;
  icon_color?: string | null;
  volume_percent: number;
  muted: boolean;
}

export interface OutputDevice {
  index: number;
  name: string;
  description: string;
}

/** Parametric EQ band shapes (mirrors Rust EqBandKind). */
export type EqBandKind = "peaking" | "low_shelf" | "high_shelf" | "low_pass" | "high_pass";

/** One parametric EQ band (mirrors Rust EqBand). */
export interface EqBand {
  kind: EqBandKind;
  freq_hz: number;
  /** Ignored by low_pass/high_pass. */
  gain_db: number;
  /** Peaking/LP/HP: filter Q. Shelves: RBJ shelf slope. */
  q: number;
}

/** A channel's parametric EQ (mirrors Rust EqConfig). */
export interface EqConfig {
  enabled: boolean;
  /** Headroom trim applied before the band cascade (dB). */
  preamp_db: number;
  bands: EqBand[];
}

export const MAX_EQ_BANDS = 10;
export const EQ_GAIN_RANGE_DB = 24;
export const EQ_FREQ_MIN_HZ = 20;
export const EQ_FREQ_MAX_HZ = 20000;

/** The Sonar-style starting layout (mirrors Rust default_eq_bands). */
export const DEFAULT_EQ_BANDS: EqBand[] = [
  { kind: "low_shelf", freq_hz: 100, gain_db: 0, q: 0.71 },
  { kind: "peaking", freq_hz: 500, gain_db: 0, q: 1 },
  { kind: "peaking", freq_hz: 1500, gain_db: 0, q: 1 },
  { kind: "peaking", freq_hz: 5000, gain_db: 0, q: 1 },
  { kind: "high_shelf", freq_hz: 10000, gain_db: 0, q: 0.71 },
];

/** A channel's EQ when it has never been configured. */
export function defaultEqConfig(): EqConfig {
  return {
    enabled: false,
    preamp_db: 0,
    bands: DEFAULT_EQ_BANDS.map((b) => ({ ...b })),
  };
}

/** App history entry (mirrors Rust SeenApp). */
export interface SeenApp {
  match_prop: string;
  match_value: string;
  display_name: string;
  icon_name: string | null;
  icon_path: string | null;
  /** Unix seconds of the last sighting. */
  last_seen: number;
  ignored: boolean;
  assigned_sink: string | null;
  alias: string | null;
}

export type InputKind = "software" | "hardware";
export interface FxChain {
  high_pass_hz: number | null;
  eq_enabled: boolean;
  gate_enabled: boolean;
  compressor_enabled: boolean;
  limiter_enabled: boolean;
  gate_threshold_db: number;
  compressor_threshold_db: number;
  compressor_ratio: number;
  limiter_ceiling_db: number;
}
export const FX_DEFAULTS = {
  gate_threshold_db: -40,
  compressor_threshold_db: -18,
  compressor_ratio: 3,
  limiter_ceiling_db: -1,
} as const;
export interface RoutingInput {
  id: string;
  label: string;
  icon: string | null;
  icon_color: string | null;
  kind: InputKind;
  source_name: string;
  volume_percent: number;
  muted: boolean;
  fx: FxChain;
  order: number;
}
export interface OutputBinding {
  device: string;
  enabled: boolean;
}
export interface RoutingMix {
  id: string;
  label: string;
  icon: string | null;
  icon_color: string | null;
  volume_percent: number;
  muted: boolean;
  output_bindings: OutputBinding[];
  order: number;
}
export interface RouteCell {
  enabled: boolean;
  send_percent: number;
  muted: boolean;
}
export interface RoutingModel {
  version: number;
  inputs: RoutingInput[];
  mixes: RoutingMix[];
  routes: Record<string, Record<string, RouteCell>>;
  /** One input heard alone; `restore` holds every input's earlier mute. */
  solo?: { input: string; restore: Record<string, boolean> } | null;
}

/** Profile listing entry; trigger_device auto-loads the profile. */
export interface ProfileInfo {
  name: string;
  trigger_device: string | null;
}

/** Sent as sink_name to unassign a stream (backend moves it to the default sink). */
export const UNASSIGNED = "";

/** Unity: no level in WaveSink ever amplifies. */
export const MAX_VOLUME = 100;
/** Mix output binding that follows the desktop's default output device
 *  (mirrors Rust SYSTEM_DEFAULT_OUTPUT). */
export const SYSTEM_DEFAULT_OUTPUT = "@default";
/** Node name of the always-on master mix (carries every channel). */
export interface HotkeyShortcut {
  id: string;
  description: string;
  /** Human-readable key; empty when nothing is bound yet. */
  trigger: string;
}

export interface HotkeyStatus {
  backend: "portal" | "x11" | "none";
  shortcuts: HotkeyShortcut[];
}
