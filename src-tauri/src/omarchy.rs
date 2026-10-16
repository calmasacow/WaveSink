//! User-owned shell integration, embedded so portable and native installs agree.
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use serde_json::json;

const INSTALLER: &str = include_str!("../../packaging/omarchy/integration.py");

pub fn configure(action: &str, automatic: bool) -> Result<serde_json::Value, String> {
    let config = crate::persistence::config_root().ok_or("Cannot locate desktop configuration")?;
    let omarchy = std::env::var_os("OMARCHY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/share/omarchy"));
    if !omarchy.join("shell/Ui/Panel.qml").is_file() || !config.join("omarchy/shell.json").is_file()
    {
        return Ok(json!({"supported": false, "enabled": false}));
    }
    let payload = if action == "install" {
        let executable = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(std::env::current_exe)
            .map_err(|e| e.to_string())?;
        Some(
            json!({
                "manifest.json": include_str!("../../packaging/omarchy/manifest.json"),
                "Panel.qml": include_str!("../../packaging/omarchy/Panel.qml"),
                "LevelRow.qml": include_str!("../../packaging/omarchy/LevelRow.qml"),
                "Model.js": include_str!("../../packaging/omarchy/Model.js"),
                "integration.py": INSTALLER,
                "launch.json": json!({ "command": [executable] }).to_string(),
            })
            .to_string(),
        )
    } else {
        None
    };
    let mut command = Command::new("python3");
    command.args(["-c", INSTALLER, action]);
    if automatic {
        command.arg("--automatic");
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("Omarchy panel setup: {e}"))?;
    let input_result = if let Some(payload) = payload {
        child
            .stdin
            .take()
            .ok_or("Panel setup stdin unavailable")?
            .write_all(payload.as_bytes())
    } else {
        Ok(())
    };
    drop(child.stdin.take());
    let result = child.wait_with_output().map_err(|e| e.to_string())?;
    input_result.map_err(|e| format!("Panel setup input: {e}"))?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_string());
    }
    serde_json::from_slice(&result.stdout).map_err(|e| format!("Panel setup response: {e}"))
}

#[tauri::command]
pub fn get_omarchy_panel() -> Result<serde_json::Value, String> {
    configure("status", false)
}

#[tauri::command]
pub fn set_omarchy_panel(enabled: bool) -> Result<serde_json::Value, String> {
    configure(if enabled { "install" } else { "remove" }, false)
}
