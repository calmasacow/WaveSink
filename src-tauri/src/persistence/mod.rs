pub mod active;
pub mod aliases;
pub mod assignments;
pub mod autostart;
pub mod buses;
pub mod channels;
pub mod eq;
pub mod eq_presets;
pub mod hotkeys;
pub mod mic;
pub mod outputs;
pub mod prefs;
pub mod profiles;
pub mod seen;
pub mod wireplumber;

/// Seconds since the Unix epoch, or 0 if the clock predates it.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// User config root. Release builds always resolve `$XDG_CONFIG_HOME`; the
/// test override below does not exist in them.
pub fn config_root() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    if let Some(root) = testing::config_root_override() {
        return Some(root);
    }
    dirs::config_dir()
}

/// WaveSink's config directory. On first access, atomically move a legacy
/// `sink` directory when no `wavesink` directory already exists.
pub fn app_config_dir() -> Result<std::path::PathBuf, crate::error::SinkError> {
    let root = config_root().ok_or_else(|| {
        crate::error::SinkError::Config("cannot resolve user config directory".into())
    })?;
    let active = root.join("wavesink");
    if active.try_exists()? {
        return Ok(active);
    }
    let legacy = root.join("sink");
    if legacy.try_exists()? {
        if let Err(error) = std::fs::rename(legacy, &active) {
            // Another startup thread may have completed the same migration.
            if !active.try_exists()? {
                return Err(error.into());
            }
        }
    }
    Ok(active)
}

/// Redirects [`config_root`] per thread, so a test can exercise a real save
/// path without writing into the developer's own config.
#[cfg(test)]
pub mod testing {
    use std::cell::RefCell;
    use std::path::PathBuf;

    thread_local! {
        static ROOT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }

    pub fn config_root_override() -> Option<PathBuf> {
        ROOT.with(|r| r.borrow().clone())
    }

    /// A private config root for this test, removed when the guard drops.
    pub struct TempConfig(PathBuf);

    impl TempConfig {
        pub fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "sink-test-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temp config root");
            ROOT.with(|r| *r.borrow_mut() = Some(dir.clone()));
            Self(dir)
        }
    }

    impl TempConfig {
        /// The override is per thread, so a thread a test spawns must adopt
        /// the root or it would write into the developer's own config.
        pub fn adopt(&self) -> AdoptedRoot {
            ROOT.with(|r| *r.borrow_mut() = Some(self.0.clone()));
            AdoptedRoot
        }
    }

    pub struct AdoptedRoot;

    impl Drop for AdoptedRoot {
        fn drop(&mut self) {
            ROOT.with(|r| *r.borrow_mut() = None);
        }
    }

    impl Drop for TempConfig {
        fn drop(&mut self) {
            ROOT.with(|r| *r.borrow_mut() = None);
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// Create WaveSink's config directory (and parents) with owner-only access -
/// routing rules and app history are nobody else's business.
pub fn ensure_private_dir(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write `contents` to `path` atomically via a temp file, fsync, then rename -
/// a crash mid-write leaves the old file or the new one, never truncated.
pub fn write_atomic(path: &std::path::Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Temp file in the same directory so the rename stays on one filesystem
    // (a cross-device rename is not atomic).
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(contents.as_ref())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Factory reset: delete everything WaveSink saved - the active config
/// directory and the WirePlumber routing rules.
pub fn wipe_all() -> Result<(), crate::error::SinkError> {
    let dir = app_config_dir()?;
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    if let Ok(conf) = wireplumber::conf_path() {
        if conf.exists() {
            std::fs::remove_file(&conf)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_overwrites_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!(
            "sink-write-atomic-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let path = dir.join("cfg.json");

        write_atomic(&path, b"first").expect("first write");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");

        // A shorter follow-up must fully replace, not overlay, the old bytes.
        write_atomic(&path, b"second, longer contents").expect("overwrite");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "second, longer contents"
        );

        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        assert!(
            !std::path::Path::new(&tmp).exists(),
            "temp file must not linger"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn app_config_dir_migrates_complete_legacy_tree() {
        let root = testing::TempConfig::new("migrate-config");
        let legacy = config_root().unwrap().join("sink");
        std::fs::create_dir_all(legacy.join("profiles")).unwrap();
        std::fs::write(legacy.join("prefs.json"), "prefs").unwrap();
        std::fs::write(legacy.join("profiles/gaming.json"), "profile").unwrap();

        let active = app_config_dir().unwrap();

        assert_eq!(active, config_root().unwrap().join("wavesink"));
        assert!(!legacy.exists());
        assert_eq!(
            std::fs::read_to_string(active.join("prefs.json")).unwrap(),
            "prefs"
        );
        assert_eq!(
            std::fs::read_to_string(active.join("profiles/gaming.json")).unwrap(),
            "profile"
        );
        drop(root);
    }

    #[test]
    fn app_config_dir_prefers_wavesink_and_leaves_legacy_untouched() {
        let root = testing::TempConfig::new("both-configs");
        let config = config_root().unwrap();
        let legacy = config.join("sink");
        let active = config.join("wavesink");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&active).unwrap();
        std::fs::write(legacy.join("state"), "legacy").unwrap();
        std::fs::write(active.join("state"), "current").unwrap();

        assert_eq!(app_config_dir().unwrap(), active);
        assert_eq!(
            std::fs::read_to_string(legacy.join("state")).unwrap(),
            "legacy"
        );
        assert_eq!(
            std::fs::read_to_string(active.join("state")).unwrap(),
            "current"
        );
        drop(root);
    }
}
