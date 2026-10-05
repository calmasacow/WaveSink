//! Local control socket for stream controllers and other surfaces (the
//! OpenDeck plugin): `$XDG_RUNTIME_DIR/wavesink/control.sock`, private to
//! the user. The protocol is in [`protocol`].
//!
//! Each connection is served on its own thread, one request at a time, so a
//! client's relative changes (dial ticks) apply in order. A broadcaster
//! thread pushes the state to subscribers whenever it changes, whoever
//! changed it; the level emitter pushes meter frames through [`Hub`].

pub mod protocol;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use protocol::{Effect, Levels, MeterKey, Snapshot};

/// Overrides the socket path, for tests and unusual sessions.
pub const SOCKET_ENV: &str = "WAVESINK_CONTROL_SOCKET";

/// How often the broadcaster looks for changes made elsewhere (the UI, the
/// tray, hotkeys) while someone is subscribed.
const STATE_POLL: Duration = Duration::from_millis(100);
/// With nobody subscribed there is nothing to look for.
const IDLE_POLL: Duration = Duration::from_secs(1);
/// The profile list is a directory read; it changes rarely.
const PROFILE_POLL: Duration = Duration::from_secs(1);
/// A client that stops reading is dropped rather than stalling the others.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// Meter rate for remote surfaces while the window itself shows none.
pub const REMOTE_METER_FPS: u8 = 15;

pub fn socket_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(SOCKET_ENV) {
        return Some(PathBuf::from(path));
    }
    let base = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("wavesink"),
        None => {
            use std::os::unix::fs::MetadataExt;
            let uid = std::fs::metadata("/proc/self").ok()?.uid();
            std::env::temp_dir().join(format!("wavesink-{uid}"))
        }
    };
    Some(base.join("control.sock"))
}

struct Client {
    id: u64,
    writer: Arc<Mutex<UnixStream>>,
    state: bool,
    levels: bool,
}

/// Subscribers, and the wake-up for the broadcaster.
#[derive(Default)]
pub struct Hub {
    clients: Mutex<Vec<Client>>,
    next_id: AtomicU64,
    /// Set when this socket changed something, so the next push is
    /// immediate instead of waiting for the poll.
    dirty: Mutex<bool>,
    wake: Condvar,
    meter_keys: Mutex<Vec<MeterKey>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn send_line(writer: &Mutex<UnixStream>, line: &str) -> std::io::Result<()> {
    let mut stream = lock(writer);
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")
}

impl Hub {
    fn add(&self, writer: Arc<Mutex<UnixStream>>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        lock(&self.clients).push(Client {
            id,
            writer,
            state: false,
            levels: false,
        });
        id
    }

    fn remove(&self, id: u64) {
        lock(&self.clients).retain(|c| c.id != id);
    }

    fn subscribe(&self, id: u64, state: bool, levels: bool) {
        if let Some(client) = lock(&self.clients).iter_mut().find(|c| c.id == id) {
            client.state = state;
            client.levels = levels;
        }
    }

    fn wants_state(&self) -> bool {
        lock(&self.clients).iter().any(|c| c.state)
    }

    /// Whether any surface is showing meters, so they must keep running even
    /// while WaveSink sits in the tray.
    pub fn wants_levels(&self) -> bool {
        lock(&self.clients).iter().any(|c| c.levels)
    }

    fn mark_dirty(&self) {
        *lock(&self.dirty) = true;
        self.wake.notify_all();
    }

    /// Send to every client the filter picks; drop the ones that fail.
    fn broadcast(&self, line: &str, pick: impl Fn(&Client) -> bool) {
        let targets: Vec<(u64, Arc<Mutex<UnixStream>>)> = lock(&self.clients)
            .iter()
            .filter(|c| pick(c))
            .map(|c| (c.id, c.writer.clone()))
            .collect();
        for (id, writer) in targets {
            if send_line(&writer, line).is_err() {
                self.remove(id);
            }
        }
    }

    /// One meter frame from the level emitter.
    pub fn publish_levels(&self, raw: &HashMap<String, [f32; 2]>) {
        let levels = Levels::from_raw(&lock(&self.meter_keys), raw);
        self.broadcast(&protocol::event("levels", &levels), |c| c.levels);
    }
}

/// Bind the socket and start serving. Failure only costs remote control, so
/// it is logged rather than fatal.
pub fn start(app: AppHandle) {
    let Some(path) = socket_path() else {
        eprintln!("wavesink: no runtime directory for the control socket");
        return;
    };
    let listener = match bind(&path) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("wavesink: control socket {}: {e}", path.display());
            return;
        }
    };
    let broadcaster = app.clone();
    std::thread::spawn(move || broadcast_state(broadcaster));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let app = app.clone();
                    std::thread::spawn(move || serve(app, stream));
                }
                Err(e) => eprintln!("wavesink: control socket accept: {e}"),
            }
        }
    });
}

fn bind(path: &std::path::Path) -> std::io::Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = path.parent() {
        crate::persistence::ensure_private_dir(dir)?;
    }
    // Single-instance: a socket file left here belongs to a WaveSink that
    // is gone.
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

fn serve(app: AppHandle, stream: UnixStream) {
    let hub = app.state::<Hub>();
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let writer = Arc::new(Mutex::new(stream));
    let id = hub.add(writer.clone());
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    loop {
        line.clear();
        match reader
            .by_ref()
            .take(protocol::MAX_LINE)
            .read_line(&mut line)
        {
            Ok(0) | Err(_) => break,
            Ok(_) if !line.ends_with('\n') => break, // over-long line
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let request = protocol::parse(&line);
        let result = request.call.and_then(|call| {
            let (value, effect) = protocol::dispatch(app.state::<AppState>().inner(), call)?;
            apply(&app, &hub, id, &writer, effect);
            Ok(value)
        });
        if send_line(&writer, &protocol::reply(&request.id, result)).is_err() {
            break;
        }
    }
    hub.remove(id);
}

/// Tell the UI and the other subscribers what a call changed.
fn apply(app: &AppHandle, hub: &Hub, id: u64, writer: &Mutex<UnixStream>, effect: Effect) {
    match effect {
        Effect::None => {}
        Effect::Mixer => {
            let _ = app.emit("control-changed", ());
            hub.mark_dirty();
        }
        Effect::Profile(name) => {
            crate::refresh_tray(app);
            let _ = app.emit("profile-changed", name);
            hub.mark_dirty();
        }
        Effect::ShowWindow => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }
        Effect::Subscribe { state, levels } => {
            hub.subscribe(id, state, levels);
            if levels {
                if let Ok(mixer) = app.state::<AppState>().lock_mixer() {
                    *lock(&hub.meter_keys) = protocol::meter_keys(&mixer.routing);
                }
            }
            hub.mark_dirty();
            if state {
                // The first frame goes out before the reply, so a client
                // never acts on a subscription it has no state for.
                if let Ok(snapshot) = Snapshot::current(app.state::<AppState>().inner(), None) {
                    let _ = send_line(writer, &protocol::event("state", &snapshot));
                }
            }
        }
    }
}

/// Push the state whenever it changes. Changes from this socket wake it at
/// once; changes from anywhere else are found by the poll.
fn broadcast_state(app: AppHandle) {
    let hub = app.state::<Hub>();
    let mut last: Option<Snapshot> = None;
    let mut profiles: Vec<String> = Vec::new();
    let mut profiles_read: Option<Instant> = None;
    loop {
        let (wants_state, wants_levels) = (hub.wants_state(), hub.wants_levels());
        let poll = if wants_state || wants_levels {
            STATE_POLL
        } else {
            IDLE_POLL
        };
        let forced = {
            let dirty = lock(&hub.dirty);
            let (mut dirty, _) = hub
                .wake
                .wait_timeout_while(dirty, poll, |d| !*d)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *dirty)
        };
        let state = app.state::<AppState>();
        // Meter keys follow Audio FX and new inputs, with or without a
        // state subscriber.
        if wants_levels {
            if let Ok(mixer) = state.lock_mixer() {
                *lock(&hub.meter_keys) = protocol::meter_keys(&mixer.routing);
            }
        }
        if !wants_state {
            last = None;
            continue;
        }
        if forced || profiles_read.is_none_or(|at| at.elapsed() >= PROFILE_POLL) {
            if let Ok(names) = protocol::profile_names() {
                profiles = names;
            }
            profiles_read = Some(Instant::now());
        }
        let Ok(snapshot) = Snapshot::current(state.inner(), Some(profiles.clone())) else {
            continue;
        };
        if last.as_ref() != Some(&snapshot) {
            hub.broadcast(&protocol::event("state", &snapshot), |c| c.state);
            last = Some(snapshot);
        }
    }
}
