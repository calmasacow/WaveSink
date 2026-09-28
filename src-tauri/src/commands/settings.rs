use std::collections::BTreeMap;
use std::fs;

use serde::Serialize;
use tauri::State;

use crate::persistence::autostart;
use crate::persistence::prefs::{DeviceLabelStyle, Prefs};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct BackendInfo {
    /// True = native PipeWire backend; false = pactl subprocess fallback.
    pub native: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OmarchyTheme {
    pub name: String,
    pub colors: BTreeMap<String, String>,
}

fn parse_omarchy_colors(raw: &str) -> Option<BTreeMap<String, String>> {
    let colors: BTreeMap<_, _> = raw
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let value = value.trim().trim_matches('"');
            (value.len() == 7
                && value.starts_with('#')
                && value[1..].chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| (key.trim().to_owned(), value.to_owned()))
        })
        .collect();
    [
        "accent",
        "background",
        "foreground",
        "bright_foreground",
        "red",
        "yellow",
        "green",
    ]
    .iter()
    .all(|key| colors.contains_key(*key))
    .then_some(colors)
}

#[tauri::command]
pub fn get_omarchy_theme() -> Option<OmarchyTheme> {
    let state = dirs::home_dir()?.join(".local/state/omarchy/current");
    let name = fs::read_to_string(state.join("theme.name")).ok()?;
    let name = name.trim();
    (!name.is_empty()
        && name.len() <= 80
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    .then_some(OmarchyTheme {
        name: name.to_owned(),
        colors: parse_omarchy_colors(&fs::read_to_string(state.join("theme/colors.toml")).ok()?)?,
    })
}

#[tauri::command]
pub fn get_backend_info(state: State<'_, AppState>) -> BackendInfo {
    BackendInfo {
        native: state.backend_native,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_omarchy_colors;

    #[test]
    fn parses_a_complete_omarchy_palette() {
        let colors = parse_omarchy_colors(
            "accent = \"#7aa2f7\"\nbackground = \"#1a1b26\"\nforeground = \"#a9b1d6\"\nbright_foreground = \"#c0caf5\"\nred = \"#f7768e\"\nyellow = \"#e0af68\"\ngreen = \"#9ece6a\"",
        )
        .expect("palette");
        assert_eq!(colors["accent"], "#7aa2f7");
    }

    #[test]
    fn rejects_incomplete_or_invalid_palettes() {
        assert!(parse_omarchy_colors("accent = \"#nothex\"").is_none());
    }
}

#[tauri::command]
pub fn get_autostart() -> bool {
    autostart::is_enabled()
}

/// Enable/disable the systemd user unit for autostart on login.
#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<bool, String> {
    let result = if enabled {
        autostart::enable()
    } else {
        autostart::disable()
    };
    result.map_err(|e| e.to_string())?;
    Ok(autostart::is_enabled())
}

#[tauri::command]
pub fn get_prefs(state: State<'_, AppState>) -> Result<Prefs, String> {
    Ok(state.lock_mixer()?.prefs.clone())
}

/// Set the device naming style. Existing nodes keep their labels until
/// they are recreated (restart or rename).
#[tauri::command]
pub fn set_device_label_style(
    state: State<'_, AppState>,
    style: DeviceLabelStyle,
) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.device_label_style = style;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Toggle "start minimized" (boot to tray when autostarting); rewrites the
/// systemd unit when autostart is already enabled so the flag stays in sync.
#[tauri::command]
pub fn set_start_minimized(state: State<'_, AppState>, minimized: bool) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.start_minimized = minimized;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())?;
    if autostart::is_enabled() {
        autostart::enable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Show or hide the title-bar balance slider.
#[tauri::command]
pub fn set_balance_visible(state: State<'_, AppState>, visible: bool) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.show_balance = visible;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Pick the two channels the balance slider blends.
#[tauri::command]
pub fn set_balance_channels(
    state: State<'_, AppState>,
    a: Option<String>,
    b: Option<String>,
) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.balance_a = a;
        mixer.prefs.balance_b = b;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Mark the first-run tutorial as completed (never shown again, until a
/// factory reset).
#[tauri::command]
pub fn set_onboarded(state: State<'_, AppState>) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.onboarded = true;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Factory reset: tear down our audio nodes, wipe every saved file, undo
/// autostart, and relaunch as if freshly installed.
#[tauri::command]
pub fn reset_app(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    // Best-effort teardown - the relaunch recreates everything anyway.
    for err in state.teardown_virtual_sinks() {
        eprintln!("wavesink: reset teardown: {err}");
    }
    let _ = autostart::disable();
    crate::persistence::wipe_all().map_err(|e| e.to_string())?;
    app.restart()
}

#[derive(Debug, Clone, Serialize)]
pub struct DefaultDevices {
    pub output: Option<String>,
    pub input: Option<String>,
}

/// Current system default output/input device node names.
#[tauri::command]
pub fn get_default_devices(state: State<'_, AppState>) -> Result<DefaultDevices, String> {
    let (output, input) = state
        .backend
        .get_default_devices()
        .map_err(|e| e.to_string())?;
    Ok(DefaultDevices { output, input })
}

/// Set the system default output device.
#[tauri::command]
pub fn set_default_output(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state
        .backend
        .set_default_output(&name)
        .map_err(|e| e.to_string())
}

/// Set the system default input device.
#[tauri::command]
pub fn set_default_input(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state
        .backend
        .set_default_input(&name)
        .map_err(|e| e.to_string())
}
