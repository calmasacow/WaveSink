mod audio;
mod commands;
mod error;
mod hotkeys;
mod mixer;
mod persistence;
pub(crate) mod routing_model;
mod state;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::menu::{CheckMenuItem, Menu, MenuItem}; // CheckMenuItem: profile rows
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};

use audio::backend::AudioBackend;
use audio::pw_native::levels::LevelStore;
use audio::pw_native::PipeWireBackend;
use state::AppState;

const SET_MIX_VOLUME_FLAG: &str = "--set-mix-volume";

/// Meter rates for the level emitter, set from prefs at startup and by
/// `set_meter_prefs`: focused fps, and the unfocused policy.
static METER_FPS: AtomicU8 = AtomicU8::new(30);
static METER_UNFOCUSED: AtomicU8 = AtomicU8::new(0);

pub(crate) fn set_meter_rates(fps: u8, unfocused: persistence::prefs::MeterUnfocused) {
    use persistence::prefs::MeterUnfocused;
    METER_FPS.store(fps, Ordering::Relaxed);
    let policy = match unfocused {
        MeterUnfocused::Reduced => 0,
        MeterUnfocused::Full => 1,
        MeterUnfocused::Off => 2,
    };
    METER_UNFOCUSED.store(policy, Ordering::Relaxed);
}

/// The rate meters run at right now; 0 = paused.
fn meter_fps(onscreen: bool, focused: bool) -> u8 {
    meter_fps_for(
        METER_FPS.load(Ordering::Relaxed),
        METER_UNFOCUSED.load(Ordering::Relaxed),
        onscreen,
        focused,
    )
}

fn meter_fps_for(fps: u8, unfocused_policy: u8, onscreen: bool, focused: bool) -> u8 {
    if !onscreen {
        return 0;
    }
    if focused {
        return fps;
    }
    match unfocused_policy {
        1 => fps,
        2 => 0,
        _ => fps.min(persistence::prefs::METER_REDUCED_FPS),
    }
}

/// `--set-mix-volume <mix> <percent>` from a forwarded argv, percent clamped
/// to the mix range.
fn parse_set_mix_volume(argv: &[String]) -> Option<(String, u8)> {
    let at = argv.iter().position(|a| a == SET_MIX_VOLUME_FLAG)?;
    let name = argv.get(at + 1)?;
    let percent: u16 = argv.get(at + 2)?.parse().ok()?;
    let volume = percent.min(u16::from(commands::routing::MAX_VOLUME)) as u8;
    Some((name.clone(), volume))
}

/// How long startup waits for PipeWire before giving up.
const PIPEWIRE_CONNECT_ATTEMPTS: u32 = 20;
const PIPEWIRE_CONNECT_RETRY: Duration = Duration::from_millis(500);

fn connect_pipewire() -> Result<PipeWireBackend, error::SinkError> {
    let mut attempt = 1;
    loop {
        match PipeWireBackend::new() {
            Ok(backend) => return Ok(backend),
            Err(e) if attempt >= PIPEWIRE_CONNECT_ATTEMPTS => return Err(e),
            Err(_) => {
                attempt += 1;
                std::thread::sleep(PIPEWIRE_CONNECT_RETRY);
            }
        }
    }
}

/// Explain a missing audio engine in a dialog, then quit: a mixer that cannot
/// reach PipeWire would only look like it works.
fn show_engine_error(detail: &str) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let message = format!(
        "WaveSink couldn't connect to PipeWire, so it can't route any audio.\n\n\
         Make sure PipeWire and WirePlumber are running \
         (systemctl --user status pipewire wireplumber), then start WaveSink again.\n\n\
         Details: {detail}"
    );
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            app.dialog()
                .message(message.clone())
                .title("WaveSink can't start")
                .kind(MessageDialogKind::Error)
                .show(move |_| handle.exit(1));
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("wavesink: could not show the startup error: {e}");
    }
}

pub fn run() {
    // WaveSink is a PipeWire graph; without it there is nothing to run. At
    // login PipeWire can still be starting, so give it a few seconds.
    let backend = match connect_pipewire() {
        Ok(backend) => backend,
        Err(e) => {
            eprintln!("wavesink: cannot connect to PipeWire: {e}");
            show_engine_error(&e.to_string());
            return;
        }
    };
    let levels = backend.levels.clone();
    let backend: Arc<dyn AudioBackend> = Arc::new(backend);
    let app_state = AppState::new(backend);

    let result = tauri::Builder::default()
        // Must stay first: a second launch would spawn a duplicate fighting
        // over the same virtual sinks.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // The Omarchy audio panel forwards its mix sliders here; apply
            // and persist without raising the window.
            if let Some((name, volume)) = parse_set_mix_volume(&argv) {
                match commands::buses::set_bus_volume(app.state(), name, volume) {
                    Ok(()) => {
                        let _ = app.emit("buses-changed", ());
                    }
                    Err(e) => eprintln!("wavesink: --set-mix-volume failed: {e}"),
                }
                return;
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .manage(hotkeys::Hotkeys::default())
        .invoke_handler(tauri::generate_handler![
            commands::devices::get_virtual_devices,
            commands::devices::get_app_streams,
            commands::devices::get_output_devices,
            commands::devices::init_virtual_devices,
            commands::devices::teardown_virtual_devices,
            commands::apps::get_seen_apps,
            commands::apps::set_app_ignored,
            commands::apps::forget_app,
            commands::apps::set_app_assignment,
            commands::channels::add_channel,
            commands::channels::rename_channel,
            commands::channels::reorder_channels,
            commands::channels::remove_channel,
            commands::channels::set_channel_icon,
            commands::channels::set_channel_icon_color,
            commands::buses::list_buses,
            commands::buses::add_bus,
            commands::buses::rename_bus,
            commands::buses::set_bus_icon,
            commands::buses::set_bus_icon_color,
            commands::buses::remove_bus,
            commands::buses::set_bus_members,
            commands::buses::set_bus_exclude,
            commands::buses::set_bus_volume,
            commands::buses::set_bus_mute,
            commands::matrix::get_routing_model,
            commands::matrix::reorder_matrix_inputs,
            commands::matrix::reorder_matrix_mixes,
            commands::matrix::add_hardware_input,
            commands::matrix::set_route_cell,
            commands::matrix::set_input_level,
            commands::matrix::update_hardware_input,
            commands::matrix::remove_hardware_input,
            commands::matrix::set_mix_outputs,
            commands::matrix::set_input_fx,
            commands::routing::route_app_to_channel,
            commands::routing::set_channel_volume,
            commands::routing::toggle_channel_mute,
            commands::routing::set_app_volume,
            commands::routing::rename_app,
            commands::devices::get_input_devices,
            commands::eq::get_channel_eq_configs,
            commands::eq::set_channel_eq,
            commands::eq::list_eq_presets,
            commands::eq::save_user_eq_preset,
            commands::eq::delete_user_eq_preset,
            commands::eq::export_channel_eq,
            commands::eq::export_channel_eq_to_file,
            commands::eq::import_eq_config,
            commands::eq::import_eq_file,
            commands::profiles::list_profiles,
            commands::profiles::load_profile,
            commands::profiles::delete_profile,
            commands::profiles::set_profile_trigger,
            commands::profiles::create_blank_profile,
            commands::profiles::get_active_profile,
            commands::settings::get_omarchy_theme,
            commands::settings::get_autostart,
            commands::settings::set_autostart,
            commands::settings::get_prefs,
            commands::settings::set_onboarded,
            commands::settings::set_start_minimized,
            commands::settings::set_meter_prefs,
            commands::settings::reset_app,
            commands::hotkeys::get_hotkeys,
            commands::hotkeys::configure_hotkeys,
            commands::hotkeys::set_hotkey_binding,
        ])
        .setup(move |app| {
            if let Err(error) = persistence::autostart::migrate_legacy_unit() {
                eprintln!("wavesink: autostart migration failed: {error}");
            }
            build_tray(app)?;
            {
                let prefs = app.state::<AppState>().lock_mixer()?.prefs.clone();
                set_meter_rates(prefs.meter_fps, prefs.meter_unfocused);
            }
            hotkeys::start(app.handle().clone());
            // The window starts hidden (config) to avoid a flash; show it
            // now unless launched with --minimized (autostart-to-tray).
            // A first launch from --set-mix-volume shouldn't pop the window
            // either; the mixes don't exist yet, so there is nothing to set.
            let minimized =
                std::env::args().any(|a| a == "--minimized" || a == SET_MIX_VOLUME_FLAG);
            if !minimized {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                }
            }
            spawn_level_emitter(app.handle().clone(), levels);
            persistence::wireplumber::remove_stale();
            spawn_route_enforcer(app.handle().clone());
            Ok(())
        })
        // Close button hides to tray instead of quitting.
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(e) = window.hide() {
                    eprintln!("wavesink: failed to hide window: {e}");
                }
            }
        })
        .run(tauri::generate_context!());

    if let Err(e) = result {
        eprintln!("wavesink: fatal error while running tauri application: {e}");
        std::process::exit(1);
    }
}

/// The gap before a new stream lands on its channel is audible, and a
/// native check is cheap enough to run this often.
const ROUTE_ENFORCE_INTERVAL: Duration = Duration::from_millis(200);

/// Enforces assignments from the backend; the UI poll pauses in the tray.
fn spawn_route_enforcer(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let interval = ROUTE_ENFORCE_INTERVAL;
        let mut last_error: Option<String> = None;
        let mut failures: u32 = 0;
        loop {
            let pause = if failures >= 3 {
                interval * 10
            } else {
                interval
            };
            std::thread::sleep(pause);
            let state = handle.state::<AppState>();
            match commands::devices::refresh_streams(state.inner()) {
                Ok(_) => {
                    last_error = None;
                    failures = 0;
                }
                Err(e) => {
                    failures = failures.saturating_add(1);
                    if last_error.as_deref() != Some(e.as_str()) {
                        eprintln!("wavesink: auto-route enforcement failed: {e}");
                        last_error = Some(e);
                    }
                }
            }
        }
    });
}

/// Streams meter peak levels to the UI as `levels` events, at the meter rate.
/// Peaks are drained (read-and-reset), so silence decays to zero.
fn spawn_level_emitter(handle: tauri::AppHandle, levels: Arc<LevelStore>) {
    std::thread::spawn(move || {
        let mut prev_all_zero = false;
        let mut meters_on: Option<bool> = None;
        loop {
            // The app's dominant state is sitting in the tray; don't lock the
            // registry, serialize a map, and wake a webview nobody can see.
            let (onscreen, focused) = handle
                .get_webview_window("main")
                .map(|w| {
                    let shown =
                        w.is_visible().unwrap_or(true) && !w.is_minimized().unwrap_or(false);
                    (shown, w.is_focused().unwrap_or(true))
                })
                .unwrap_or((true, true));
            let fps = meter_fps(onscreen, focused);
            // Pause the meter streams themselves while hidden (or set off
            // while unfocused), so metering costs nothing then.
            let active = fps > 0;
            if meters_on != Some(active) {
                let backend = handle.state::<AppState>().backend.clone();
                match backend.set_meters_active(active) {
                    Ok(()) => meters_on = Some(active),
                    Err(e) => eprintln!("wavesink: set_meters_active: {e}"),
                }
                // Settle the UI's meters to zero rather than freezing them.
                if !active && onscreen {
                    let _ = handle.emit("levels", HashMap::<String, [f32; 2]>::new());
                }
            }
            if !active {
                // Force a fresh frame when metering resumes.
                prev_all_zero = false;
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            // The UI interpolates and decays between frames.
            std::thread::sleep(Duration::from_millis(1000 / u64::from(fps)));
            // The meter registry is dynamic (user-defined channels + mic).
            let payload: HashMap<String, [f32; 2]> = levels
                .names()
                .into_iter()
                .map(|(name, slot)| (name, [levels.drain(slot, 0), levels.drain(slot, 1)]))
                .collect();
            // Emit the first all-zero frame so the meters settle to zero, then
            // go quiet until sound returns instead of pushing silence at 10 Hz.
            let all_zero = payload.values().all(|[l, r]| *l < 1e-4 && *r < 1e-4);
            if all_zero && prev_all_zero {
                continue;
            }
            prev_all_zero = all_zero;
            if handle.emit("levels", &payload).is_err() {
                // App is shutting down.
                break;
            }
        }
    });
}

/// Build the tray menu, including the live Profiles submenu (check on the
/// active profile). Rebuilt via `refresh_tray` whenever profiles change.
fn build_tray_menu(app: &tauri::AppHandle) -> Result<Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    use tauri::menu::{IsMenuItem, Submenu};

    let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;

    let active = app
        .state::<AppState>()
        .lock_mixer()
        .ok()
        .and_then(|m| m.active_profile.clone());
    let profile_items: Vec<CheckMenuItem<tauri::Wry>> = persistence::profiles::list()
        .unwrap_or_default()
        .into_iter()
        .map(|info| {
            CheckMenuItem::with_id(
                app,
                format!("profile:{}", info.name),
                &info.name,
                true,
                active.as_deref() == Some(info.name.as_str()),
                None::<&str>,
            )
        })
        .collect::<Result<_, _>>()?;
    let profile_refs: Vec<&dyn IsMenuItem<tauri::Wry>> = profile_items
        .iter()
        .map(|i| i as &dyn IsMenuItem<tauri::Wry>)
        .collect();
    let profiles_menu = Submenu::with_items(app, "Profiles", true, &profile_refs)?;

    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    Ok(Menu::with_items(app, &[&show, &profiles_menu, &quit])?)
}

/// Rebuild the tray menu (called after anything that changes profiles or
/// their active state).
pub(crate) fn refresh_tray(app: &tauri::AppHandle) {
    if let Some(tray) = app.tray_by_id("wavesink-tray") {
        match build_tray_menu(app) {
            Ok(menu) => {
                if let Err(e) = tray.set_menu(Some(menu)) {
                    eprintln!("wavesink: tray menu refresh failed: {e}");
                }
            }
            Err(e) => eprintln!("wavesink: tray menu rebuild failed: {e}"),
        }
    }
}

fn build_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let menu = build_tray_menu(app.handle())?;

    let icon = tauri::image::Image::from_bytes(include_bytes!("../../assets/WaveSinkTray.png"))?;

    let tray = TrayIconBuilder::with_id("wavesink-tray")
        .icon(icon.clone())
        .tooltip("WaveSink")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| {
            let id = event.id.as_ref();
            if let Some(name) = id.strip_prefix("profile:") {
                // Switch profiles straight from the tray; tell the UI.
                match commands::profiles::load_profile(app.clone(), app.state(), name.to_string()) {
                    Ok(()) => {
                        let _ = app.emit("profile-changed", name);
                    }
                    Err(e) => eprintln!("wavesink: tray profile switch failed: {e}"),
                }
                return;
            }
            match id {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "quit" => {
                    // Clean up our virtual sinks before exiting. Best-effort:
                    // log failures but never block quitting.
                    let state = app.state::<AppState>();
                    for err in state.teardown_virtual_sinks() {
                        eprintln!("wavesink: teardown: {err}");
                    }
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;
    tray.set_icon(Some(icon))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{meter_fps_for, parse_set_mix_volume};

    #[test]
    fn meters_follow_focus_policy() {
        // Hidden: always paused.
        assert_eq!(meter_fps_for(30, 1, false, true), 0);
        // Focused: the chosen rate.
        assert_eq!(meter_fps_for(20, 0, true, true), 20);
        // Unfocused: reduced, full, or off.
        assert_eq!(meter_fps_for(30, 0, true, false), 10);
        assert_eq!(meter_fps_for(30, 1, true, false), 30);
        assert_eq!(meter_fps_for(30, 2, true, false), 0);
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_set_mix_volume() {
        let argv = args(&["wavesink", "--set-mix-volume", "sink_bus_stream", "80"]);
        assert_eq!(
            parse_set_mix_volume(&argv),
            Some(("sink_bus_stream".to_string(), 80))
        );
    }

    #[test]
    fn clamps_set_mix_volume() {
        let argv = args(&["wavesink", "--set-mix-volume", "sink_bus_chat", "400"]);
        assert_eq!(
            parse_set_mix_volume(&argv),
            Some(("sink_bus_chat".to_string(), 100))
        );
    }

    #[test]
    fn rejects_incomplete_set_mix_volume() {
        assert_eq!(parse_set_mix_volume(&args(&["wavesink"])), None);
        assert_eq!(
            parse_set_mix_volume(&args(&["wavesink", "--set-mix-volume", "sink_bus_chat"])),
            None
        );
        assert_eq!(
            parse_set_mix_volume(&args(&["wavesink", "--set-mix-volume", "x", "-5"])),
            None
        );
    }
}
