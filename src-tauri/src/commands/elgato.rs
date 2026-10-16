use tauri::State;

use crate::{audio::elgato, state::AppState};

fn card(state: &AppState, source_name: &str) -> Result<Option<u32>, String> {
    state
        .backend
        .list_input_devices()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|d| d.name == source_name)
        .map(|d| d.alsa_card)
        .ok_or_else(|| "Hardware source disconnected; refresh devices".into())
}

#[tauri::command]
pub fn get_elgato_controls(
    state: State<'_, AppState>,
    source_name: String,
) -> Result<Option<elgato::Controls>, String> {
    let Some(card) = card(&state, &source_name)? else {
        return Ok(None);
    };
    elgato::read(card)
}

#[tauri::command]
pub fn set_elgato_control(
    state: State<'_, AppState>,
    source_name: String,
    token: String,
    setting: elgato::Setting,
    phantom_confirmed: bool,
) -> Result<elgato::Controls, String> {
    let card = card(&state, &source_name)?.ok_or("Not a USB audio source")?;
    elgato::write(card, &token, setting, phantom_confirmed)
}
