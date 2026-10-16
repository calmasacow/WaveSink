import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Controls {
  token: string;
  model: string;
  serial: string;
  gain_db: number;
  max_gain_db: number;
  muted: boolean;
  headphone_db: number;
  min_headphone_db: number;
  phantom: boolean | null;
  low_impedance: boolean | null;
  monitor_mix: number | null;
}

type Setting =
  | { control: "gain" | "headphones" | "monitor_mix"; value: number }
  | { control: "mute" | "phantom" | "low_impedance"; value: boolean };

/** Keyed by source at the call site: a pending edit never follows selection. */
export function ElgatoControls({ sourceName }: Readonly<{ sourceName: string }>) {
  const [controls, setControls] = useState<Controls | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [refresh, setRefresh] = useState(0);
  // ponytail: refresh on demand; add polling when live hardware-dial sync is needed.
  useEffect(() => {
    let live = true;
    setBusy(true);
    setError("");
    setControls(null);
    void invoke<Controls | null>("get_elgato_controls", { sourceName })
      .then((result) => {
        if (live) setControls(result);
        return null;
      })
      .catch((e: unknown) => {
        if (live) setError(String(e));
      })
      .finally(() => {
        if (live) setBusy(false);
      });
    return () => {
      live = false;
    };
    // oxlint-disable-next-line react/exhaustive-effect-dependencies -- refresh explicitly reloads hardware state.
  }, [sourceName, refresh]);

  const change = async (setting: Setting) => {
    if (!controls || busy) return;
    const phantom = setting.control === "phantom";
    if (
      phantom &&
      !window.confirm(
        `${setting.value ? "Enable" : "Disable"} 48 V on ${controls.model} (${controls.serial || "selected device"})?\n\nCheck your microphone and cable manufacturer's phantom-power instructions. Reduce monitoring volume first. Only enable 48 V for equipment that requires and supports it.`,
      )
    )
      return;
    setBusy(true);
    setError("");
    try {
      setControls(
        await invoke<Controls>("set_elgato_control", {
          sourceName,
          token: controls.token,
          setting,
          phantomConfirmed: phantom,
        }),
      );
    } catch (e) {
      setError(String(e));
      setControls(null);
    } finally {
      setBusy(false);
    }
  };

  if (!controls && !error)
    return busy ? (
      <p className="modal-text" role="status">
        Reading device controls…
      </p>
    ) : null;
  return (
    <section className="elgato-controls" aria-label="Elgato hardware controls">
      <div className="elgato-heading">
        <strong>{controls?.model ?? "Hardware controls"}</strong>
        <button
          type="button"
          className="modal-btn"
          disabled={busy}
          onClick={() => setRefresh((n) => n + 1)}
        >
          Refresh
        </button>
      </div>
      {controls && (
        <>
          {controls.serial && <p className="modal-text">Serial: {controls.serial}</p>}
          <p className="modal-text">
            Applies immediately to hardware. Shared by all apps; separate from mix levels.
          </p>
          <fieldset disabled={busy}>
            <legend className="sr-only">Device settings</legend>
            <HardwareNumber
              key={`gain-${controls.gain_db}`}
              label="Microphone gain"
              value={controls.gain_db}
              min={0}
              max={controls.max_gain_db}
              step={controls.model === "XLR Dock MK.2" ? 1 : 0.5}
              unit="dB"
              onSet={(value) => void change({ control: "gain", value })}
            />
            <label className="elgato-toggle">
              <input
                type="checkbox"
                checked={controls.muted}
                onChange={(e) => void change({ control: "mute", value: e.target.checked })}
              />
              Hardware mute
            </label>
            {controls.phantom !== null && (
              <label className="elgato-toggle">
                <input
                  type="checkbox"
                  checked={controls.phantom}
                  onChange={(e) => void change({ control: "phantom", value: e.target.checked })}
                />
                48 V phantom power
              </label>
            )}
            <HardwareNumber
              key={`hp-${controls.headphone_db}`}
              label="Headphone level"
              value={controls.headphone_db}
              min={controls.min_headphone_db}
              max={0}
              step={0.25}
              unit="dB"
              onSet={(value) => void change({ control: "headphones", value })}
            />
            {controls.low_impedance !== null && (
              <label className="elgato-toggle">
                <input
                  type="checkbox"
                  checked={controls.low_impedance}
                  onChange={(e) =>
                    void change({ control: "low_impedance", value: e.target.checked })
                  }
                />
                Low impedance headphones
              </label>
            )}
            {controls.monitor_mix !== null && (
              <HardwareNumber
                key={`mix-${controls.monitor_mix}`}
                label="Monitor mix (0 = mic, 100 = PC)"
                value={controls.monitor_mix}
                min={0}
                max={100}
                step={1}
                unit="%"
                onSet={(value) => void change({ control: "monitor_mix", value })}
              />
            )}
          </fieldset>
        </>
      )}
      {error && (
        <p className="modal-error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

function HardwareNumber({
  label,
  value,
  min,
  max,
  step,
  unit,
  onSet,
}: Readonly<{
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  unit: string;
  onSet: (value: number) => void;
}>) {
  const [draft, setDraft] = useState(String(value));
  return (
    <form
      className="elgato-number"
      onSubmit={(e) => {
        e.preventDefault();
        if (draft.trim() && Number.isFinite(Number(draft))) onSet(Number(draft));
      }}
    >
      <label>
        {label}
        <input
          className="menu-input"
          type="number"
          required
          min={min}
          max={max}
          step={step}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
        />
      </label>
      <span>{unit}</span>
      <button type="submit" className="modal-btn" disabled={Number(draft) === value}>
        Set
      </button>
    </form>
  );
}
