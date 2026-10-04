import { useEffect, useRef, useSyncExternalStore } from "react";
import type { RefObject } from "react";
import type { Levels } from "../store/mixer";
import { perceptual } from "./audio";

/**
 * Live meters, kept out of React state. The backend pushes peaks at ~30 fps;
 * each meter element is registered once and repainted from one shared
 * requestAnimationFrame loop by setting CSS variables, so a meter frame never
 * re-renders a component.
 *
 * `--meter` is the smoothed level and `--meter-peak` a short peak-hold tick,
 * both as percentages of the track width.
 *
 * Two scales: Standard (the default) maps amplitude by square root, which
 * reads like a familiar 0-100 volume bar; Pro Audio Metering maps -60..0 dBFS
 * linearly and shows dB labels and peak readouts.
 */

export type MeterUnfocused = "reduced" | "full" | "off";
export interface MeterConfig {
  pro: boolean;
  fps: number;
  unfocused: MeterUnfocused;
}

/** Bottom of the Pro scale. */
const PRO_FLOOR_DB = -60;
/** The rate an unfocused window drops to when set to "reduced". */
const REDUCED_FPS = 10;

let config: MeterConfig = { pro: false, fps: 30, unfocused: "reduced" };
const configListeners = new Set<() => void>();
let focused = typeof document === "undefined" || document.hasFocus();

/** Apply meter settings (from prefs, or a change in Settings). */
export function setMeterConfig(next: MeterConfig) {
  config = next;
  if (typeof document !== "undefined") {
    document.documentElement.dataset.meterScale = next.pro ? "pro" : "standard";
  }
  for (const listener of configListeners) listener();
  wake();
}

/** The current meter settings as React state. */
export function useMeterConfig(): MeterConfig {
  return useSyncExternalStore(
    (listener) => {
      configListeners.add(listener);
      return () => configListeners.delete(listener);
    },
    () => config,
  );
}

if (typeof window !== "undefined") {
  window.addEventListener("focus", () => {
    focused = true;
    wake();
  });
  window.addEventListener("blur", () => {
    focused = false;
  });
}

/** Paint rate right now, mirroring the backend's focus policy. */
function paintFps(): number {
  if (focused || config.unfocused === "full") return config.fps;
  // Off still paints at a low rate so meters settle to zero, then idles.
  return Math.min(REDUCED_FPS, config.fps);
}

/** Track position (0..1) of a linear peak amplitude on the active scale. */
export function meterPosition(amplitude: number, pro: boolean): number {
  if (!pro) return perceptual(amplitude);
  if (amplitude <= 0) return 0;
  const db = 20 * Math.log10(amplitude);
  return Math.min(1, Math.max(0, (db - PRO_FLOOR_DB) / -PRO_FLOOR_DB));
}

/** A slider percent as gain in dB, on the backend's cubic volume curve. */
export function gainDb(percent: number): string {
  if (percent <= 0) return "−∞ dB";
  const db = 60 * Math.log10(percent / 100);
  return (Math.abs(db) < 0.05 ? "0.0" : db.toFixed(1).replace("-", "−")) + " dB";
}

/** A Pro track position back as a dBFS readout. */
function positionDbfs(position: number): string {
  if (position <= 0) return "< −60 dBFS";
  const db = PRO_FLOOR_DB + position * -PRO_FLOOR_DB;
  return db.toFixed(1).replace("-", "−") + " dBFS";
}

let latest: Levels = {};
const listeners = new Set<() => void>();

/** Linear peak amplitude (0..1) of a level key, louder channel wins. */
export function peakOf(key: string | null | undefined): number {
  if (!key) return 0;
  const level = latest[key];
  return level ? Math.max(level[0], level[1]) : 0;
}

/** Feed a `levels` event from the backend. */
export function pushLevels(levels: Levels) {
  latest = levels;
  for (const listener of listeners) listener();
  wake();
}

/** A mix send percent as linear gain, matching the backend's cubic curve. */
export function sendGain(percent: number): number {
  const f = Math.max(0, percent) / 100;
  return f * f * f;
}

/** Fast attack, slower release; the tick holds a peak, then falls. */
const RELEASE_PER_MS = 0.0035;
const PEAK_HOLD_MS = 900;
const PEAK_FALL_PER_MS = 0.0012;
/** Stop the frame loop once everything has settled this long. */
const IDLE_MS = 600;

interface Meter {
  source: () => number;
  shown: number;
  peak: number;
  peakAt: number;
  /** Last values written, so an unchanged meter is never repainted. */
  meterCss: string;
  peakCss: string;
  /** Pro peak readout, updated only while the pointer is over the meter. */
  readout: RefObject<HTMLElement | null> | null;
  hovered: boolean;
  readoutText: string;
}

const meters = new Map<HTMLElement, Meter>();
let raf = 0;
let last = 0;
let quietSince = 0;

function wake() {
  if (raf === 0 && meters.size > 0) {
    last = performance.now();
    raf = requestAnimationFrame(frame);
  }
}

function frame(now: number) {
  // Paint at the data rate, not the display's refresh rate.
  if (now - last < 1000 / paintFps() - 2) {
    raf = requestAnimationFrame(frame);
    return;
  }
  const dt = Math.min(100, now - last);
  last = now;
  let busy = false;
  for (const [el, m] of meters) {
    const target = meterPosition(m.source(), config.pro);
    m.shown = target >= m.shown ? target : Math.max(target, m.shown - RELEASE_PER_MS * dt);
    if (m.shown >= m.peak) {
      m.peak = m.shown;
      m.peakAt = now;
    } else if (now - m.peakAt > PEAK_HOLD_MS) {
      m.peak = Math.max(m.shown, m.peak - PEAK_FALL_PER_MS * dt);
    }
    if (m.shown > 0.001 || m.peak > 0.001) busy = true;
    const meterCss = (m.shown * 100).toFixed(1) + "%";
    const peakCss = (m.peak * 100).toFixed(1) + "%";
    if (meterCss !== m.meterCss) {
      m.meterCss = meterCss;
      el.style.setProperty("--meter", meterCss);
    }
    if (peakCss !== m.peakCss) {
      m.peakCss = peakCss;
      el.style.setProperty("--meter-peak", peakCss);
    }
    const readout = m.readout?.current;
    if (readout && m.hovered && config.pro) {
      const text = "Peak " + positionDbfs(m.peak);
      if (text !== m.readoutText) {
        m.readoutText = text;
        readout.textContent = text;
      }
    }
  }
  if (busy) quietSince = now;
  if (meters.size === 0 || now - quietSince > IDLE_MS) {
    raf = 0;
    return;
  }
  raf = requestAnimationFrame(frame);
}

/**
 * Attach a meter to an element (usually a range input). `source` returns a
 * linear peak amplitude and is re-read every frame, so it can derive a value
 * (an input's level times a send gain) without any extra backend stream.
 */
export function useMeter<T extends HTMLElement>(
  source: () => number,
  readout: RefObject<HTMLElement | null> | null = null,
) {
  const ref = useRef<T>(null);
  const sourceRef = useRef(source);
  sourceRef.current = source;
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const meter: Meter = {
      source: () => sourceRef.current(),
      shown: 0,
      peak: 0,
      peakAt: 0,
      meterCss: "",
      peakCss: "",
      readout,
      hovered: false,
      readoutText: "",
    };
    const enter = () => {
      meter.hovered = true;
      meter.readoutText = "";
      wake();
    };
    const leave = () => {
      meter.hovered = false;
    };
    el.addEventListener("pointerenter", enter);
    el.addEventListener("pointerleave", leave);
    meters.set(el, meter);
    wake();
    return () => {
      el.removeEventListener("pointerenter", enter);
      el.removeEventListener("pointerleave", leave);
      meters.delete(el);
      el.style.removeProperty("--meter");
      el.style.removeProperty("--meter-peak");
    };
    // The readout ref is stable for the element's lifetime.
    // oxlint-disable-next-line react/exhaustive-deps
  }, []);
  return ref;
}

/**
 * The raw level for one key as React state, for the few components that
 * render a level themselves. Re-renders on every frame while it changes, so
 * keep it to small leaf components.
 */
export function useLevel(key: string): [number, number] | undefined {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => latest[key],
  );
}
