import { useState, type MouseEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useMixerStore } from "../../store/mixer";
import { useTheme } from "../../store/theme";
import { ConfirmModal } from "../ConfirmModal";
import { Ms } from "../Icons";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";
import appIcon from "../../../assets/WaveSinkIcon.png";
import { ProfileMenu } from "./ProfileMenu";

interface EngineReset {
  command: string;
  detected: string;
  custom: boolean;
}

/**
 * Frameless headerbar: brand, screen, engine status, window controls.
 * The close button hides to tray (intercepted on the Rust side).
 * Right-clicking the engine status offers a PipeWire reset.
 */
export function TitleBar({ screen }: Readonly<{ screen: string }>) {
  const win = getCurrentWindow();
  const error = useMixerStore((s) => s.error);
  const initialized = useMixerStore((s) => s.initialized);
  const omarchy = useTheme((s) => s.omarchy);
  const [menuOpen, setMenuOpen] = useState(false);
  const [reset, setReset] = useState<EngineReset | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [resetting, setResetting] = useState(false);

  let status = "Starting…";
  if (resetting) status = "Restarting audio…";
  else if (error) status = "Engine error";
  else if (initialized) status = "Engine running";

  const openMenu = (event: MouseEvent) => {
    event.preventDefault();
    setMenuOpen(true);
    // Detected per open: an override saved in Settings applies right away.
    void invoke<EngineReset>("get_engine_reset")
      .then(setReset)
      .catch(() => setReset(null));
  };

  const resetEngine = async () => {
    setResetting(true);
    try {
      // On success WaveSink is relaunched and this window goes away.
      await invoke("reset_audio_engine");
    } catch (e) {
      useMixerStore.setState({ error: String(e) });
      setResetting(false);
    }
  };

  return (
    <header data-tauri-drag-region className="headerbar">
      <div data-tauri-drag-region className="hb-brand">
        <img className="hb-logo" src={appIcon} alt="" />
        <div data-tauri-drag-region className="hb-title">
          WaveSink
        </div>
      </div>
      <div data-tauri-drag-region className="hb-sub">
        {screen}
      </div>
      <div data-tauri-drag-region className="hb-spacer" />
      <ProfileMenu />
      <div style={{ position: "relative" }}>
        <div
          className={"hb-status" + (error && !resetting ? " err" : "")}
          onContextMenu={openMenu}
          title="Right-click for engine options"
        >
          <span className="dot" />
          {status}
        </div>
        <Popover
          open={menuOpen}
          onClose={() => setMenuOpen(false)}
          side="bottom"
          align="end"
          style={{ minWidth: 220 }}
        >
          <MenuItem
            icon="restart_alt"
            title={reset?.command}
            onClick={() => {
              setMenuOpen(false);
              setConfirming(true);
            }}
          >
            Reset audio engine…
          </MenuItem>
        </Popover>
      </div>
      <div className="wctl">
        {!omarchy && (
          <>
            <button
              type="button"
              className="wbtn"
              aria-label="Minimize"
              onClick={() => void win.minimize()}
            >
              <Ms name="remove" />
            </button>
            <button
              type="button"
              className="wbtn"
              aria-label="Maximize"
              onClick={() => void win.toggleMaximize()}
            >
              <Ms name="crop_square" style={{ fontSize: 13 }} />
            </button>
          </>
        )}
        <button
          type="button"
          className="wbtn close"
          aria-label="Close (hide to tray)"
          title="Hides to tray"
          onClick={() => void win.close()}
        >
          <Ms name="close" />
        </button>
      </div>
      <ConfirmModal
        open={confirming}
        onClose={() => setConfirming(false)}
        title="Reset audio engine?"
        confirmLabel="Reset engine"
        onConfirm={() => void resetEngine()}
      >
        Restarts PipeWire with <code>{reset?.command ?? "the detected command"}</code>, then
        relaunches WaveSink. Audio drops out for a few seconds. Change the command in Settings.
      </ConfirmModal>
    </header>
  );
}
