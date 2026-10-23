//! "Reset audio engine": restart the PipeWire stack, then WaveSink with it
//! (our virtual nodes die with the old PipeWire). How to restart PipeWire
//! differs per distro, so the command is detected on the running system;
//! a command saved in prefs overrides the detection.

use std::path::Path;
use std::process::Command;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::persistence::autostart::UNIT_NAME;
use crate::state::AppState;

/// The user units a stock PipeWire session runs, in restart order.
const PIPEWIRE_UNITS: [&str; 4] = [
    "pipewire.service",
    "pipewire-pulse.service",
    "wireplumber.service",
    "pipewire-media-session.service",
];

#[derive(Debug, Clone, Serialize)]
pub struct EngineReset {
    /// What a reset runs right now.
    pub command: String,
    /// What this system would run without an override.
    pub detected: String,
    /// The command comes from the user's override.
    pub custom: bool,
}

fn on_path(binary: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| is_executable(&dir.join(binary))))
        .unwrap_or(false)
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn systemctl_show(unit: &str, property: &str) -> Option<String> {
    let out = Command::new("systemctl")
        .args(["--user", "show", "-p", property, "--value", unit])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn unit_loaded(unit: &str) -> bool {
    systemctl_show(unit, "LoadState").as_deref() == Some("loaded")
}

/// Pick the restart command from what this system has.
fn detect_with(has_binary: impl Fn(&str) -> bool, unit_loaded: impl Fn(&str) -> bool) -> String {
    // Omarchy wraps the restart (and its own bar refresh) in its CLI.
    if has_binary("omarchy") {
        return "omarchy restart audio".into();
    }
    if has_binary("omarchy-restart-pipewire") {
        return "omarchy-restart-pipewire".into();
    }
    let units: Vec<&str> = PIPEWIRE_UNITS
        .iter()
        .copied()
        .filter(|unit| unit_loaded(unit))
        .map(|unit| unit.trim_end_matches(".service"))
        .collect();
    if units.is_empty() {
        // systemd could not be asked; the common unit set is the best guess.
        return "systemctl --user restart pipewire pipewire-pulse wireplumber".into();
    }
    format!("systemctl --user restart {}", units.join(" "))
}

pub fn detect_command() -> String {
    detect_with(on_path, unit_loaded)
}

fn resolve(custom: Option<&str>) -> EngineReset {
    let detected = detect_command();
    match custom.map(str::trim).filter(|c| !c.is_empty()) {
        Some(command) => EngineReset {
            command: command.to_owned(),
            detected,
            custom: true,
        },
        None => EngineReset {
            command: detected.clone(),
            detected,
            custom: false,
        },
    }
}

/// True when this process is the main process of the autostart unit, so
/// systemd (not us) must relaunch it.
fn managed_by_unit() -> bool {
    systemctl_show(UNIT_NAME, "MainPID").as_deref() == Some(&std::process::id().to_string())
}

#[tauri::command]
pub fn get_engine_reset(state: State<'_, AppState>) -> Result<EngineReset, String> {
    let custom = state.lock_mixer()?.prefs.pipewire_reset_command.clone();
    Ok(resolve(custom.as_deref()))
}

/// Save (or, with an empty/absent command, clear) the reset override.
#[tauri::command]
pub fn set_engine_reset_command(
    state: State<'_, AppState>,
    command: Option<String>,
) -> Result<EngineReset, String> {
    let command = command
        .map(|c| c.trim().to_owned())
        .filter(|c| !c.is_empty());
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.pipewire_reset_command = command;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())?;
    Ok(resolve(prefs.pipewire_reset_command.as_deref()))
}

/// Restart PipeWire with the resolved command, then relaunch WaveSink: via
/// its systemd unit when it runs under one, otherwise in-process.
#[tauri::command]
pub async fn reset_audio_engine(app: AppHandle) -> Result<(), String> {
    let custom = app
        .state::<AppState>()
        .lock_mixer()?
        .prefs
        .pipewire_reset_command
        .clone();
    let command = resolve(custom.as_deref()).command;
    let out = tauri::async_runtime::spawn_blocking(move || {
        Command::new("sh").args(["-c", &command]).output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("Audio engine reset: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "Audio engine reset failed ({}): {}",
            out.status,
            stderr.trim()
        ));
    }
    if managed_by_unit() {
        // --no-block: queue the job and return; systemd then stops us.
        let status = Command::new("systemctl")
            .args(["--user", "--no-block", "restart", UNIT_NAME])
            .status()
            .map_err(|e| format!("WaveSink restart: {e}"))?;
        if !status.success() {
            return Err(format!("WaveSink restart failed ({status})"));
        }
        return Ok(());
    }
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::detect_with;

    #[test]
    fn omarchy_uses_its_cli() {
        assert_eq!(
            detect_with(|b| b == "omarchy", |_| true),
            "omarchy restart audio"
        );
        assert_eq!(
            detect_with(|b| b == "omarchy-restart-pipewire", |_| true),
            "omarchy-restart-pipewire"
        );
    }

    #[test]
    fn systemd_restarts_only_installed_units() {
        let loaded = ["pipewire.service", "wireplumber.service"];
        assert_eq!(
            detect_with(|_| false, |u| loaded.contains(&u)),
            "systemctl --user restart pipewire wireplumber"
        );
        assert_eq!(
            detect_with(|_| false, |u| u != "pipewire-media-session.service"),
            "systemctl --user restart pipewire pipewire-pulse wireplumber"
        );
    }

    #[test]
    fn unknown_systems_fall_back_to_the_common_units() {
        assert_eq!(
            detect_with(|_| false, |_| false),
            "systemctl --user restart pipewire pipewire-pulse wireplumber"
        );
    }
}
