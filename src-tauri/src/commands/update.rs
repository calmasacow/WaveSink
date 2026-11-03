//! Update checks against the GitHub releases API, and the one install kind we
//! can safely update ourselves.
//!
//! Only an AppImage is self-updating: it is a single user-owned file we can
//! replace. Every packaged install (pacman/dpkg/rpm) needs root, and a
//! background GUI service is the wrong place to ask for it, so those get the
//! exact command to run instead. Nix is declarative and is never told to
//! update in place.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const LATEST_URL: &str = "https://api.github.com/repos/calmasacow/WaveSink/releases/latest";
const RELEASES_BASE: &str = "https://github.com/calmasacow/WaveSink/releases";
/// GitHub rejects API requests without one.
const USER_AGENT: &str = concat!("WaveSink/", env!("CARGO_PKG_VERSION"));

/// How the running copy was installed, which decides what an update can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallKind {
    /// A single user-owned file we can replace in place.
    AppImage,
    Pacman,
    Dpkg,
    Rpm,
    /// Declarative: the user's flake or profile owns the version.
    Nix,
    /// A dev build (`cargo run`) or something we don't recognise.
    Unknown,
}

impl InstallKind {
    /// Whether WaveSink can perform this update without asking for root.
    pub fn self_updates(self) -> bool {
        self == InstallKind::AppImage
    }

    /// The command a user runs to update a packaged install themselves.
    /// `None` where no single command is right (AppImage self-updates; a Nix
    /// install is owned by the user's configuration).
    pub fn manual_command(self, tag: &str) -> Option<String> {
        let version = tag.trim_start_matches('v');
        let url = |asset: &str| format!("{RELEASES_BASE}/download/{tag}/{asset}");
        match self {
            InstallKind::Pacman => Some(format!(
                "sudo pacman -U {}",
                url(&format!("wavesink-bin-{version}-1-x86_64.pkg.tar.zst"))
            )),
            InstallKind::Dpkg => Some(format!(
                "sudo dpkg -i {}",
                url(&format!("wavesink_{version}_amd64.deb"))
            )),
            InstallKind::Rpm => Some(format!(
                "sudo rpm -U {}",
                url(&format!("wavesink-{version}-1.x86_64.rpm"))
            )),
            InstallKind::AppImage | InstallKind::Nix | InstallKind::Unknown => None,
        }
    }
}

/// Classify from the executable path, which is how each installer lays it out.
/// `APPIMAGE` is set by the AppImage runtime and wins: inside one, the exe path
/// points at a temporary mount, not the image.
///
/// `has_db` names the package database to look for; it is injected so the
/// ordering can be tested without the host's real `/var/lib`.
fn classify(
    appimage: Option<&Path>,
    exe: Option<&Path>,
    has_db: impl Fn(&str) -> bool,
) -> InstallKind {
    if appimage.is_some() {
        return InstallKind::AppImage;
    }
    let Some(exe) = exe else {
        return InstallKind::Unknown;
    };
    let path = exe.to_string_lossy();
    // A Nix store path is immutable, so check it before the /usr/bin layouts
    // that the ordinary distro packages share.
    if path.starts_with("/nix/store/") {
        return InstallKind::Nix;
    }
    if path.ends_with(".AppImage") {
        return InstallKind::AppImage;
    }
    if exe.starts_with("/usr/bin") || exe.starts_with("/usr/local/bin") {
        // All three distro packages install to the same prefix; the package
        // manager that owns the file is what separates them. Order matters:
        // an Arch box can carry a stray /var/lib/rpm from some other tool, so
        // the native database is asked about first.
        for (db, kind) in [
            ("/var/lib/pacman/local", InstallKind::Pacman),
            ("/var/lib/dpkg/status", InstallKind::Dpkg),
            ("/var/lib/rpm", InstallKind::Rpm),
        ] {
            if has_db(db) {
                return kind;
            }
        }
    }
    InstallKind::Unknown
}

fn db_exists(path: &str) -> bool {
    Path::new(path).exists()
}

pub fn install_kind() -> InstallKind {
    classify(
        std::env::var_os("APPIMAGE").map(PathBuf::from).as_deref(),
        std::env::current_exe().ok().as_deref(),
        db_exists,
    )
}

/// Compare two dotted numeric versions. Non-numeric parts (a `-rc1` suffix)
/// sort below a bare release, so a prerelease never looks newer than the
/// stable it precedes.
fn is_newer(candidate: &str, current: &str) -> bool {
    fn parts(v: &str) -> (Vec<u64>, bool) {
        let v = v.trim().trim_start_matches('v');
        let (core, rest) = v.split_once(['-', '+']).unwrap_or((v, ""));
        (
            core.split('.').map(|p| p.parse().unwrap_or(0)).collect(),
            !rest.is_empty(),
        )
    }
    let (a, a_pre) = parts(candidate);
    let (b, b_pre) = parts(current);
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    // Equal cores: only a stable release beats a prerelease of the same number.
    b_pre && !a_pre
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    /// The version now running.
    pub current: String,
    /// Latest stable tag, absent when the check has not succeeded yet.
    pub latest: Option<String>,
    pub available: bool,
    pub install: InstallKind,
    /// True when clicking can perform the update (AppImage only).
    pub can_self_update: bool,
    /// Command for a packaged install to update by hand.
    pub command: Option<String>,
    pub notes_url: Option<String>,
    /// Unix seconds of the last successful check, for the tooltip.
    pub checked_at: Option<u64>,
    /// Why the last check failed, if it did.
    pub error: Option<String>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Pull the latest stable tag from the releases API.
///
/// Uses `curl`, already required by every installer, rather than pulling a TLS
/// stack into the binary for one request every six hours.
fn fetch_latest_tag() -> Result<String, String> {
    let output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "20",
            "-H",
            "Accept: application/vnd.github+json",
            "-A",
            USER_AGENT,
            LATEST_URL,
        ])
        .output()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if !output.status.success() {
        return Err("GitHub did not return a release".into());
    }
    let body: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("bad response: {e}"))?;
    let tag = body
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or("release has no tag")?;
    // Guard against anything that is not a plain stable tag before it reaches a
    // download URL.
    valid_tag(tag)
        .then(|| tag.to_owned())
        .ok_or_else(|| format!("unexpected release tag: {tag}"))
}

/// `vMAJOR.MINOR.PATCH` and nothing else. This is the only check standing
/// between the API response and a URL we fetch, so it stays strict.
fn valid_tag(tag: &str) -> bool {
    let Some(rest) = tag.strip_prefix('v') else {
        return false;
    };
    let mut parts = rest.split('.');
    let valid = |p: Option<&str>| {
        p.is_some_and(|p| !p.is_empty() && p.len() <= 5 && p.chars().all(|c| c.is_ascii_digit()))
    };
    valid(parts.next()) && valid(parts.next()) && valid(parts.next()) && parts.next().is_none()
}

fn status_for(
    latest: Option<String>,
    error: Option<String>,
    checked_at: Option<u64>,
) -> UpdateStatus {
    let current = env!("CARGO_PKG_VERSION").to_owned();
    let install = install_kind();
    let available = latest.as_deref().is_some_and(|t| is_newer(t, &current));
    // Nix is declarative: report the update, never offer to apply it.
    let actionable = available && install != InstallKind::Nix;
    UpdateStatus {
        command: actionable
            .then(|| latest.as_deref().and_then(|t| install.manual_command(t)))
            .flatten(),
        can_self_update: actionable && install.self_updates(),
        notes_url: latest
            .as_deref()
            .map(|t| format!("{RELEASES_BASE}/tag/{t}")),
        current,
        latest,
        available,
        install,
        checked_at,
        error,
    }
}

/// Check GitHub for a newer stable release.
#[tauri::command]
pub async fn check_for_update() -> UpdateStatus {
    // Blocking curl + file I/O belongs off the async runtime.
    tauri::async_runtime::spawn_blocking(|| match fetch_latest_tag() {
        Ok(tag) => status_for(Some(tag), None, Some(now())),
        Err(e) => status_for(None, Some(e), None),
    })
    .await
    .unwrap_or_else(|e| status_for(None, Some(e.to_string()), None))
}

/// Download the new AppImage, verify it against the release checksums, and
/// swap it in. Only valid when `can_self_update` is true.
#[tauri::command]
pub async fn perform_update(tag: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || self_update(&tag))
        .await
        .map_err(|e| e.to_string())?
}

fn self_update(tag: &str) -> Result<(), String> {
    if !valid_tag(tag) {
        return Err(format!("refusing to install unexpected tag: {tag}"));
    }
    if install_kind() != InstallKind::AppImage {
        return Err("this install cannot update itself".into());
    }
    let target = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .ok_or("APPIMAGE is not set")?;

    let version = tag.trim_start_matches('v');
    let asset = format!("wavesink_{version}_amd64.AppImage");
    let base = format!("{RELEASES_BASE}/download/{tag}");

    // Stage beside the target so the final rename stays on one filesystem.
    let dir = target.parent().ok_or("AppImage has no parent directory")?;
    let staged = dir.join(format!(".{asset}.part"));
    let cleanup = |e: String| {
        let _ = std::fs::remove_file(&staged);
        e
    };

    download(&format!("{base}/{asset}"), &staged).map_err(cleanup)?;
    let sums = fetch_text(&format!("{base}/SHA256SUMS")).map_err(cleanup)?;
    let expected = sums
        .lines()
        .find_map(|line| {
            let (sum, name) = line.split_once("  ")?;
            (name.trim() == asset).then_some(sum)
        })
        .ok_or_else(|| cleanup(format!("{asset} is not in SHA256SUMS")))?;
    let actual = sha256_file(&staged).map_err(cleanup)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(cleanup("downloaded AppImage failed checksum".into()));
    }

    // Keep it executable, then swap atomically over the running image. Linux
    // allows this: the running process keeps the old inode until it exits.
    std::fs::set_permissions(
        &staged,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .map_err(|e| cleanup(format!("could not mark the AppImage executable: {e}")))?;
    std::fs::rename(&staged, &target)
        .map_err(|e| cleanup(format!("could not replace the AppImage: {e}")))?;
    Ok(())
}

fn download(url: &str, to: &Path) -> Result<(), String> {
    let status = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--retry",
            "3",
            "--max-time",
            "600",
            "-A",
            USER_AGENT,
            url,
            "-o",
        ])
        .arg(to)
        .status()
        .map_err(|e| format!("could not run curl: {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("download failed: {url}"))
}

fn fetch_text(url: &str) -> Result<String, String> {
    let output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--retry",
            "3",
            "--max-time",
            "60",
            "-A",
            USER_AGENT,
            url,
        ])
        .output()
        .map_err(|e| format!("could not run curl: {e}"))?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
        .ok_or_else(|| format!("could not fetch {url}"))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let output = std::process::Command::new("sha256sum")
        .arg(path)
        .output()
        .map_err(|e| format!("could not run sha256sum: {e}"))?;
    if !output.status.success() {
        return Err("sha256sum failed".into());
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "sha256sum produced no digest".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions_win() {
        assert!(is_newer("v1.3.0", "1.2.1"));
        assert!(is_newer("1.2.2", "1.2.1"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("1.2.1", "1.2.1"));
        assert!(!is_newer("1.2.0", "1.2.1"));
        assert!(!is_newer("v1.0.0", "1.2.1"));
    }

    #[test]
    fn shorter_versions_compare_as_zero_padded() {
        assert!(is_newer("1.3", "1.2.9"));
        assert!(!is_newer("1.2", "1.2.1"));
        assert!(!is_newer("1.2.0", "1.2"));
    }

    #[test]
    fn a_prerelease_never_outranks_its_stable() {
        assert!(!is_newer("1.3.0-rc1", "1.3.0"));
        assert!(is_newer("1.3.0", "1.3.0-rc1"));
        // but it is still newer than the release before it
        assert!(is_newer("1.3.0-rc1", "1.2.1"));
    }

    #[test]
    fn only_plain_stable_tags_are_accepted() {
        assert!(valid_tag("v1.2.1"));
        assert!(valid_tag("v10.0.300"));
        assert!(!valid_tag("1.2.1"), "needs the v prefix");
        assert!(!valid_tag("v1.2"), "needs three parts");
        assert!(!valid_tag("v1.2.1.1"));
        assert!(!valid_tag("dev"));
        assert!(!valid_tag("v1.2.1-rc1"), "prereleases are not offered");
        assert!(!valid_tag("v1.2.1/../../etc"), "no path traversal");
        assert!(!valid_tag("v1.2.1; rm -rf /"));
        assert!(!valid_tag("v999999.0.0"), "absurd parts are rejected");
    }

    /// A host with only the named package databases present.
    fn dbs<'a>(present: &'a [&'static str]) -> impl Fn(&str) -> bool + 'a {
        move |path| present.contains(&path)
    }

    #[test]
    fn appimage_env_beats_the_exe_path() {
        let kind = classify(
            Some(Path::new("/home/me/Apps/WaveSink.AppImage")),
            Some(Path::new("/tmp/.mount_abc/usr/bin/wavesink")),
            dbs(&[]),
        );
        assert_eq!(kind, InstallKind::AppImage);
    }

    #[test]
    fn nix_store_is_never_a_distro_package() {
        let kind = classify(
            None,
            Some(Path::new("/nix/store/abc123-wavesink/bin/wavesink")),
            dbs(&["/var/lib/dpkg/status"]),
        );
        assert_eq!(kind, InstallKind::Nix);
        assert!(!kind.self_updates());
        assert_eq!(kind.manual_command("v1.3.0"), None);
    }

    #[test]
    fn unknown_installs_offer_nothing() {
        let kind = classify(
            None,
            Some(Path::new("/home/me/src/wavesink/target/debug/wavesink")),
            dbs(&["/var/lib/pacman/local"]),
        );
        assert_eq!(kind, InstallKind::Unknown);
        assert!(!kind.self_updates());
        assert_eq!(kind.manual_command("v1.3.0"), None);
    }

    #[test]
    fn each_package_database_picks_its_own_manager() {
        let exe = Some(Path::new("/usr/bin/wavesink"));
        for (db, want) in [
            ("/var/lib/pacman/local", InstallKind::Pacman),
            ("/var/lib/dpkg/status", InstallKind::Dpkg),
            ("/var/lib/rpm", InstallKind::Rpm),
        ] {
            assert_eq!(classify(None, exe, dbs(&[db])), want, "{db}");
        }
    }

    /// Observed on a real Arch host: /var/lib/rpm existed alongside pacman's
    /// database, so a looser ordering would have offered an rpm command for a
    /// pacman install.
    #[test]
    fn a_stray_rpm_database_does_not_outrank_pacman() {
        let kind = classify(
            None,
            Some(Path::new("/usr/bin/wavesink")),
            dbs(&["/var/lib/pacman/local", "/var/lib/rpm"]),
        );
        assert_eq!(kind, InstallKind::Pacman);
    }

    #[test]
    fn packaged_installs_get_a_pinned_download_url() {
        let pacman = InstallKind::Pacman.manual_command("v1.3.0").unwrap();
        assert!(pacman.starts_with("sudo pacman -U https://"));
        assert!(pacman.ends_with("wavesink-bin-1.3.0-1-x86_64.pkg.tar.zst"));
        assert!(InstallKind::Dpkg
            .manual_command("v1.3.0")
            .unwrap()
            .ends_with("wavesink_1.3.0_amd64.deb"));
        assert!(InstallKind::Rpm
            .manual_command("v1.3.0")
            .unwrap()
            .ends_with("wavesink-1.3.0-1.x86_64.rpm"));
    }

    #[test]
    fn only_the_appimage_claims_to_self_update() {
        assert!(InstallKind::AppImage.self_updates());
        for kind in [
            InstallKind::Pacman,
            InstallKind::Dpkg,
            InstallKind::Rpm,
            InstallKind::Nix,
            InstallKind::Unknown,
        ] {
            assert!(!kind.self_updates(), "{kind:?} must not self-update");
        }
    }

    #[test]
    fn a_failed_check_reports_no_update() {
        let status = status_for(None, Some("offline".into()), None);
        assert!(!status.available);
        assert!(!status.can_self_update);
        assert_eq!(status.command, None);
        assert_eq!(status.error.as_deref(), Some("offline"));
    }

    #[test]
    fn the_running_version_is_not_an_update() {
        let current = env!("CARGO_PKG_VERSION");
        let status = status_for(Some(format!("v{current}")), None, Some(1));
        assert!(!status.available);
        assert!(!status.can_self_update);
        assert_eq!(status.command, None);
    }
}
