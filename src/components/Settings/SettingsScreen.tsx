import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { useMixerStore } from "../../store/mixer";
import { useTheme, themeOptions, type ThemeId } from "../../store/theme";
import { Ms } from "../Icons";
import { HotkeysSection } from "./HotkeysSection";
import { ConfirmModal } from "../ConfirmModal";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";
import { Toggle } from "../Toggle";
import {
  setMeterConfig,
  useMeterConfig,
  type MeterConfig,
  type MeterUnfocused,
} from "../../lib/meters";

const METER_FPS_CHOICES = [10, 20, 30];

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

const UNFOCUSED_CHOICES: { value: MeterUnfocused; label: string }[] = [
  { value: "reduced", label: "10 fps" },
  { value: "full", label: "Full rate" },
  { value: "off", label: "Off" },
];

/** A row with a small dropdown of fixed choices. */
function ChoiceRow<T extends string | number>({
  icon,
  title,
  sub,
  choices,
  current,
  onPick,
}: Readonly<{
  icon: string;
  title: string;
  sub: string;
  choices: { value: T; label: string }[];
  current: T;
  onPick: (value: T) => void;
}>) {
  const [open, setOpen] = useState(false);
  const currentLabel = choices.find((c) => c.value === current)?.label ?? String(current);
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
        <button type="button" className="select" onClick={() => setOpen((o) => !o)}>
          <span>{currentLabel}</span>
          <Ms name="expand_more" />
        </button>
        <Popover open={open} onClose={() => setOpen(false)} side="bottom" align="end">
          {choices.map((c) => (
            <MenuItem
              key={String(c.value)}
              icon={icon}
              selected={c.value === current}
              showCheck
              onClick={() => {
                onPick(c.value);
                setOpen(false);
              }}
            >
              {c.label}
            </MenuItem>
          ))}
        </Popover>
      </div>
    </div>
  );
}

/** Meter scale and refresh rates; saved to prefs, applied live. */
function MetersSection({ onError }: Readonly<{ onError: (error: string | null) => void }>) {
  const config = useMeterConfig();
  const save = async (next: MeterConfig) => {
    const previous = config;
    setMeterConfig(next);
    try {
      await invoke("set_meter_prefs", {
        pro: next.pro,
        fps: next.fps,
        unfocused: next.unfocused,
      });
      onError(null);
    } catch (e) {
      setMeterConfig(previous);
      onError(String(e));
    }
  };
  return (
    <>
      <div className="section-label">Meters</div>
      <div className="card" style={{ padding: "var(--sp-2)" }}>
        <div className="row">
          <div className="ricon">
            <Ms name="graphic_eq" />
          </div>
          <div className="rmain">
            <div className="rtitle">Pro Audio Metering</div>
            <div className="rsub">
              Meters in dBFS with standard color zones, levels in dB, peak readout on hover
            </div>
          </div>
          <Toggle on={config.pro} onClick={() => void save({ ...config, pro: !config.pro })} />
        </div>
        <ChoiceRow
          icon="speed"
          title="Meter frame rate"
          sub="Smoother meters cost a little more CPU while the window is open"
          choices={METER_FPS_CHOICES.map((fps) => ({ value: fps, label: `${fps} fps` }))}
          current={config.fps}
          onPick={(fps) => void save({ ...config, fps })}
        />
        <ChoiceRow
          icon="filter_center_focus"
          title="When WaveSink isn't focused"
          sub="E.g. open on a second monitor while you game"
          choices={UNFOCUSED_CHOICES}
          current={config.unfocused}
          onPick={(unfocused) => void save({ ...config, unfocused })}
        />
      </div>
    </>
  );
}

export function SettingsScreen() {
  const [panel, setPanel] = useState<{ supported: boolean; enabled: boolean } | null>(null);
  const [panelBusy, setPanelBusy] = useState(false);
  const [panelMessage, setPanelMessage] = useState<string | null>(null);
  const [panelRestoreStage, setPanelRestoreStage] = useState<0 | 1 | 2>(0);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [startMinimized, setStartMinimized] = useState(false);
  const [version, setVersion] = useState("");
  const [confirmingReset, setConfirmingReset] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const replayOnboarding = useMixerStore((s) => s.replayOnboarding);

  useEffect(() => {
    const refreshPanel = () => {
      setPanelMessage(null);
      void invoke<{ supported: boolean; enabled: boolean }>("get_omarchy_panel")
        .then(setPanel)
        .catch((e) => setError(String(e)));
    };
    refreshPanel();
    window.addEventListener("focus", refreshPanel);
    void invoke<boolean>("get_autostart").then(setAutostart);
    void invoke<{ start_minimized: boolean }>("get_prefs")
      .then((p) => {
        setStartMinimized(p.start_minimized);
      })
      .catch(() => {});
    void getVersion().then(setVersion);
    return () => window.removeEventListener("focus", refreshPanel);
  }, []);

  const configurePanel = async (enabled: boolean) => {
    if (!panel || panelBusy) return;
    setPanelBusy(true);
    setPanelMessage(null);
    try {
      const next = await invoke<{ supported: boolean; enabled: boolean }>("set_omarchy_panel", {
        enabled,
      });
      setPanel(next);
      setPanelMessage(
        next.enabled
          ? panel.enabled
            ? "WaveSink panel reapplied successfully."
            : "WaveSink panel enabled successfully."
          : "OS audio panel restored.",
      );
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setPanelBusy(false);
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

        <MetersSection onError={setError} />
        {panel?.supported && (
          <>
            <div className="section-label">Omarchy</div>
            <div className="card" style={{ padding: "var(--sp-2)" }}>
              <div className="row">
                <div className="ricon">
                  <Ms name="volume_up" />
                </div>
                <div className="rmain">
                  <div className="rtitle">
                    WaveSink audio panel
                    <span
                      className="tag"
                      style={{ color: panel.enabled ? "var(--online)" : "var(--fg-muted)" }}
                    >
                      {panel.enabled ? "Active" : "Inactive"}
                    </span>
                  </div>
                  <div className="rsub" role="status">
                    {panelBusy
                      ? "Applying panel change…"
                      : (panelMessage ??
                        (panel.enabled
                          ? "WaveSink panel is currently active in the bar."
                          : "WaveSink panel is not active. Enable it without reinstalling."))}
                  </div>
                </div>
                <button
                  type="button"
                  className="select"
                  disabled={panelBusy}
                  onClick={() => void configurePanel(true)}
                >
                  {panelBusy
                    ? "Applying…"
                    : panel.enabled
                      ? "Reapply panel"
                      : "Enable WaveSink panel"}
                </button>
              </div>
              {panel.enabled && (
                <div className="row row-sub">
                  <div className="rmain">
                    <div className="rsub">Switch back to your OS audio panel</div>
                  </div>
                  <button
                    type="button"
                    className="select"
                    disabled={panelBusy}
                    onClick={() => setPanelRestoreStage(1)}
                  >
                    Restore OS panel…
                  </button>
                </div>
              )}
            </div>
          </>
        )}

        <HotkeysSection onError={setError} />

        <div className="section-label">About</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="cable" />
            </div>
            <div className="rmain">
              <div className="rtitle">Audio engine</div>
              <div className="rsub">Native PipeWire graph (pipewire-rs)</div>
            </div>
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
        open={panelRestoreStage > 0}
        onClose={() => setPanelRestoreStage(0)}
        title={panelRestoreStage === 1 ? "Restore OS audio panel?" : "Are you sure?"}
        confirmLabel={panelRestoreStage === 1 ? "Continue" : "Restore OS panel"}
        onConfirm={() => {
          if (panelRestoreStage === 1) setPanelRestoreStage(2);
          else void configurePanel(false);
        }}
      >
        {panelRestoreStage === 1
          ? "This will restore the default OS audio panel. Any previous panel customizations will be preserved."
          : "To put this panel back, open WaveSink Settings > Omarchy and click Enable WaveSink panel. No reinstall is needed."}
      </ConfirmModal>

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
