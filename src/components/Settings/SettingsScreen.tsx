import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { useMixerStore } from "../../store/mixer";
import { useTheme, themeOptions, type ThemeId } from "../../store/theme";
import type { OutputDevice } from "../../types";
import { Ms } from "../Icons";
import { HotkeysSection } from "./HotkeysSection";
import { ConfirmModal } from "../ConfirmModal";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";
import { Toggle } from "../Toggle";

interface DefaultDevices {
  output: string | null;
  input: string | null;
}

function ThemeSwatch({ colors }: Readonly<{ colors: readonly string[] }>) {
  return (
    <span className="theme-swatch-colors" aria-hidden="true">
      {colors.map((color) => (
        <i key={color} style={{ background: color }} />
      ))}
    </span>
  );
}

function ThemePicker() {
  const [open, setOpen] = useState(false);
  const { theme, omarchy, setTheme } = useTheme();
  const options = themeOptions(omarchy);
  const selected = options.find((entry) => entry.id === theme) ?? options[0];
  const choose = (id: ThemeId) => {
    setTheme(id);
    setOpen(false);
  };

  return (
    <div className="theme-choice">
      <div className="theme-select-wrap">
        <button
          type="button"
          className="select theme-select"
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          <ThemeSwatch colors={selected.swatch} />
          <span className="theme-select-name">{selected.label}</span>
          <Ms name="expand_more" />
        </button>
        <Popover
          open={open}
          onClose={() => setOpen(false)}
          side="bottom"
          align="end"
          style={{ width: 300, maxHeight: "min(560px, calc(100vh - 32px))", overflowY: "auto" }}
        >
          {options.map((option) => (
            <button
              key={option.id}
              type="button"
              role="menuitemradio"
              aria-checked={option.id === theme}
              className={"menu-item theme-menu-item" + (option.id === theme ? " sel" : "")}
              onClick={() => choose(option.id as ThemeId)}
            >
              <ThemeSwatch colors={option.swatch} />
              <span className="theme-menu-name">{option.label}</span>
            </button>
          ))}
        </Popover>
      </div>
      <p className="theme-description">{selected.description}</p>
    </div>
  );
}

/** Card row with a device dropdown for picking a system default. */
function DeviceRow({
  icon,
  title,
  sub,
  devices,
  current,
  onPick,
}: Readonly<{
  icon: string;
  title: string;
  /** What this default is used for. */
  sub: string;
  devices: OutputDevice[];
  current: string | null;
  onPick: (name: string) => void;
}>) {
  const [open, setOpen] = useState(false);
  const currentDesc = devices.find((d) => d.name === current)?.description ?? current ?? "-";

  return (
    <div className="row">
      <div className="ricon">
        <Ms name={icon} />
      </div>
      <div className="rmain">
        <div className="rtitle">{title}</div>
        <div className="rsub">{sub}</div>
      </div>
      <div style={{ position: "relative" }}>
        <button type="button" className="select device-select" onClick={() => setOpen((o) => !o)}>
          <span className="device-select-name">{currentDesc}</span>
          <Ms name="expand_more" />
        </button>
        <Popover open={open} onClose={() => setOpen(false)} side="bottom" align="end">
          {devices.map((d) => (
            <MenuItem
              key={d.name}
              icon={icon}
              selected={d.name === current}
              showCheck
              onClick={() => {
                onPick(d.name);
                setOpen(false);
              }}
            >
              {d.description}
            </MenuItem>
          ))}
        </Popover>
      </div>
    </div>
  );
}

function engineDesc(native: boolean | null): string {
  if (native === null) return "…";
  return native
    ? "Native PipeWire (pipewire-rs) - live metering, passive routing"
    : "pactl fallback - native engine unavailable on this system";
}

export function SettingsScreen() {
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [startMinimized, setStartMinimized] = useState(false);
  const [backendNative, setBackendNative] = useState<boolean | null>(null);
  const [version, setVersion] = useState("");
  const [defaults, setDefaults] = useState<DefaultDevices>({ output: null, input: null });
  const [confirmingReset, setConfirmingReset] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const outputDevices = useMixerStore((s) => s.outputDevices);
  const inputDevices = useMixerStore((s) => s.inputDevices);
  const replayOnboarding = useMixerStore((s) => s.replayOnboarding);

  useEffect(() => {
    void invoke<boolean>("get_autostart").then(setAutostart);
    void invoke<{ native: boolean }>("get_backend_info").then((i) => setBackendNative(i.native));
    void invoke<DefaultDevices>("get_default_devices")
      .then(setDefaults)
      .catch(() => {});
    void invoke<{ start_minimized: boolean }>("get_prefs")
      .then((p) => {
        setStartMinimized(p.start_minimized);
      })
      .catch(() => {});
    void getVersion().then(setVersion);
  }, []);

  const pickDefault = async (kind: "output" | "input", name: string) => {
    try {
      await invoke(kind === "output" ? "set_default_output" : "set_default_input", { name });
      setDefaults((d) => ({ ...d, [kind]: name }));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleAutostart = async () => {
    if (autostart === null) return;
    try {
      const actual = await invoke<boolean>("set_autostart", { enabled: !autostart });
      setAutostart(actual);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleStartMinimized = async () => {
    const next = !startMinimized;
    setStartMinimized(next);
    try {
      await invoke("set_start_minimized", { minimized: next });
      setError(null);
    } catch (e) {
      setStartMinimized(!next);
      setError(String(e));
    }
  };

  return (
    <div className="content">
      <div className="screen-head">
        <h1>Settings</h1>
      </div>
      <div className="screen-scroll">
        {error && (
          <div className="error-banner" style={{ borderRadius: 8 }}>
            {error}
          </div>
        )}

        <div className="section-label">Appearance</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="palette" />
            </div>
            <div className="rmain">
              <div className="rtitle">Theme</div>
              <div className="rsub">Match the app to your desktop</div>
            </div>
            <ThemePicker />
          </div>
        </div>

        <div className="section-label">Preferences</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="label" />
            </div>
            <div className="rmain">
              <div className="rtitle">Device naming</div>
              <div className="rsub">
                System audio pickers show every virtual device as Name (WaveSink)
              </div>
            </div>
          </div>
          <DeviceRow
            icon="speaker"
            title="Default output"
            sub="Where channels set to “System default” play"
            devices={outputDevices}
            current={defaults.output}
            onPick={(name) => void pickDefault("output", name)}
          />
          <DeviceRow
            icon="mic"
            title="Default input"
            sub="The microphone the WaveSink mic chain captures"
            devices={inputDevices}
            current={defaults.input}
            onPick={(name) => void pickDefault("input", name)}
          />
          <div className="row">
            <div className="ricon">
              <Ms name="rocket_launch" />
            </div>
            <div className="rmain">
              <div className="rtitle">Start at login</div>
              <div className="rsub">systemd user service, starts with your desktop session</div>
            </div>
            {autostart !== null && <Toggle on={autostart} onClick={() => void toggleAutostart()} />}
          </div>
          {autostart && (
            <div className="row row-sub">
              <div className="ricon">
                <Ms name="dock_to_bottom" />
              </div>
              <div className="rmain">
                <div className="rtitle">Start minimized</div>
                <div className="rsub">Boot to the tray instead of opening the window</div>
              </div>
              <Toggle on={startMinimized} onClick={() => void toggleStartMinimized()} />
            </div>
          )}
        </div>

        <HotkeysSection onError={setError} />

        <div className="section-label">About</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="cable" />
            </div>
            <div className="rmain">
              <div className="rtitle">Audio engine</div>
              <div className="rsub">{engineDesc(backendNative)}</div>
            </div>
            {backendNative !== null && (
              <span className={"tag" + (backendNative ? " live" : "")}>
                {backendNative ? "native" : "fallback"}
              </span>
            )}
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="info" />
            </div>
            <div className="rmain">
              <div className="rtitle">WaveSink {version}</div>
              <div className="rsub">GPL-3.0 · config in ~/.config/wavesink</div>
            </div>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="school" />
            </div>
            <div className="rmain">
              <div className="rtitle">Tutorial</div>
              <div className="rsub">Replay the first-run tour</div>
            </div>
            <button type="button" className="select" onClick={replayOnboarding}>
              <span>Replay</span>
            </button>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="restart_alt" />
            </div>
            <div className="rmain">
              <div className="rtitle">Reset WaveSink</div>
              <div className="rsub">
                Erase all channels, mixes, profiles, app history and preferences
              </div>
            </div>
            <button type="button" className="select" onClick={() => setConfirmingReset(true)}>
              <span>Reset…</span>
            </button>
          </div>
        </div>
      </div>

      <ConfirmModal
        open={confirmingReset}
        onClose={() => setConfirmingReset(false)}
        title="Reset WaveSink?"
        confirmLabel="Reset everything"
        onConfirm={() => void invoke("reset_app").catch((e) => setError(String(e)))}
      >
        Everything you've set up - channels, mixes, profiles, app assignments, history and
        preferences - is permanently deleted, and WaveSink relaunches as if freshly installed.
      </ConfirmModal>
    </div>
  );
}
