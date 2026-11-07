//! Self-update from GitHub Releases.
//!
//! The latest (non-pre-release) GitHub release is the source of truth: its tag is the
//! version, and its asset for this platform is what gets installed. Every download is
//! checked against the SHA-256 digest GitHub publishes for the asset, and the update is
//! refused if there is no digest. New files are staged next to the program (never in a
//! temp dir, which may be on another filesystem) and swapped in with renames.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const REPO: &str = "jjanow/underkeeper";
pub const RELEASES_URL: &str = "https://github.com/jjanow/underkeeper/releases";

/// Prefix of the staging folder created next to the program while installing.
const STAGING_PREFIX: &str = ".underkeeper-update-";

/// The running version. `UNDERKEEPER_FAKE_VERSION` overrides it for testing the update path.
pub fn current_version() -> String {
    std::env::var("UNDERKEEPER_FAKE_VERSION")
        .ok()
        .filter(|v| parse_version(v).is_some())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

/// `v1.2.3`, `1.2.3` or `1.2.3-rc1` → (1, 2, 3). Missing minor/patch count as 0.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    Some((major, minor, patch))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Name of the release asset for the platform this binary was built for.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(windows) {
        Some("Underkeeper-windows-x64.exe")
    } else if cfg!(target_os = "macos") {
        Some("Underkeeper-macos-universal.zip")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("underkeeper-linux-x64.tar.gz")
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct Release {
    /// Version without the leading `v`.
    pub version: String,
    /// Release page (notes).
    pub page_url: String,
    pub asset_name: String,
    pub asset_url: String,
    /// Lowercase hex SHA-256 of the asset.
    pub sha256: String,
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(format!("underkeeper/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Asks GitHub for the latest release. `Ok(None)` means we're up to date.
pub fn check(current: &str) -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut resp = agent(Duration::from_secs(20))
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Couldn't reach GitHub: {e}"))?;
    let json: serde_json::Value = resp
        .body_mut()
        .read_json()
        .map_err(|e| format!("Unexpected reply from GitHub: {e}"))?;
    parse_release(&json, current)
}

fn parse_release(json: &serde_json::Value, current: &str) -> Result<Option<Release>, String> {
    let tag = json["tag_name"]
        .as_str()
        .ok_or("GitHub's reply has no release tag.")?;
    if !is_newer(tag, current) {
        return Ok(None);
    }
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    let want = asset_name().ok_or("Automatic updates aren't available for this system.")?;
    let asset = json["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["name"].as_str() == Some(want)))
        .ok_or_else(|| format!("Release {tag} has no download for this system ({want})."))?;
    let asset_url = asset["browser_download_url"]
        .as_str()
        .ok_or("The release download has no URL.")?
        .to_string();
    let sha256 = asset["digest"]
        .as_str()
        .and_then(|d| {
            let (algo, hex) = d.split_once(':')?;
            algo.eq_ignore_ascii_case("sha256")
                .then(|| hex.to_ascii_lowercase())
        })
        .ok_or("The release download has no SHA-256 checksum, so it can't be verified.")?;
    Ok(Some(Release {
        version,
        page_url: json["html_url"]
            .as_str()
            .unwrap_or(RELEASES_URL)
            .to_string(),
        asset_name: want.to_string(),
        asset_url,
        sha256,
    }))
}

/// What gets replaced: the executable (Windows, Linux) or the `.app` bundle (macOS).
pub fn install_target() -> Result<PathBuf, String> {
    let exe =
        std::env::current_exe().map_err(|e| format!("Can't find the running program: {e}"))?;
    let exe = exe.canonicalize().unwrap_or(exe);
    if cfg!(target_os = "macos") {
        if exe.to_string_lossy().contains("/AppTranslocation/") {
            return Err(
                "macOS is running Underkeeper from a temporary read-only copy. \
                        Move Underkeeper.app to your Applications folder, open it from there, \
                        and try again."
                    .into(),
            );
        }
        // …/Underkeeper.app/Contents/MacOS/underkeeper
        let app = exe
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
            .ok_or("Underkeeper isn't running from an .app bundle, so it can't update itself.")?;
        Ok(app.to_path_buf())
    } else {
        Ok(exe)
    }
}

/// Removes what an earlier update left behind (old Windows exe, interrupted staging folders).
pub fn cleanup_leftovers() {
    let Ok(target) = install_target() else { return };
    let _ = std::fs::remove_file(old_path(&target));
    if cfg!(target_os = "macos") {
        let _ = std::fs::remove_dir_all(old_path(&target));
    }
    let Some(dir) = target.parent() else { return };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        if e.file_name().to_string_lossy().starts_with(STAGING_PREFIX) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Where the replaced program is parked during the swap: `underkeeper.exe.old`, `Underkeeper.app.old`.
fn old_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    target.with_file_name(name)
}

/// Downloads `url` into `dest`, returning the lowercase hex SHA-256 of what was written.
fn download(url: &str, dest: &Path) -> Result<String, String> {
    let resp = agent(Duration::from_secs(600))
        .get(url)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let mut reader = resp.into_body().into_reader();
    let mut file =
        std::fs::File::create(dest).map_err(|e| format!("Can't write {}: {e}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| format!("Can't write {}: {e}", dest.display()))?;
    }
    file.sync_all()
        .map_err(|e| format!("Can't write {}: {e}", dest.display()))?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Downloads, verifies and installs `rel` over `target` (see [`install_target`]).
/// Returns the path to launch the new version with.
pub fn install(rel: &Release, target: &Path) -> Result<PathBuf, String> {
    let dir = target
        .parent()
        .ok_or("Can't find the folder Underkeeper is in.")?;
    let staging = dir.join(format!("{STAGING_PREFIX}{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| {
        format!(
            "Can't write to {} ({e}). Download the update from the Releases page instead.",
            dir.display()
        )
    })?;
    let result = install_staged(rel, target, &staging);
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn install_staged(rel: &Release, target: &Path, staging: &Path) -> Result<PathBuf, String> {
    let file = staging.join(&rel.asset_name);
    let got = download(&rel.asset_url, &file)?;
    if got != rel.sha256 {
        return Err(format!(
            "The download is corrupt (checksum mismatch: expected {}, got {got}). Nothing was changed.",
            rel.sha256
        ));
    }
    let new = unpack(&file, staging)?;
    swap(&new, target)?;
    Ok(target.to_path_buf())
}

/// Turns the downloaded asset into the thing that replaces the target.
fn unpack(file: &Path, staging: &Path) -> Result<PathBuf, String> {
    if cfg!(windows) {
        return Ok(file.to_path_buf());
    }
    let (cmd, args, out): (&str, Vec<&std::ffi::OsStr>, &str) = if cfg!(target_os = "macos") {
        (
            "ditto",
            vec![
                "-x".as_ref(),
                "-k".as_ref(),
                file.as_os_str(),
                staging.as_os_str(),
            ],
            "Underkeeper.app",
        )
    } else {
        (
            "tar",
            vec![
                "-xzf".as_ref(),
                file.as_os_str(),
                "-C".as_ref(),
                staging.as_os_str(),
            ],
            "underkeeper",
        )
    };
    let status = std::process::Command::new(cmd)
        .args(&args)
        .status()
        .map_err(|e| format!("Couldn't run {cmd} to unpack the update: {e}"))?;
    let new = staging.join(out);
    if !status.success() || !new.exists() {
        return Err(format!("Couldn't unpack the update ({cmd} failed)."));
    }
    #[cfg(unix)]
    if !cfg!(target_os = "macos") {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Couldn't make the update executable: {e}"))?;
    }
    Ok(new)
}

/// Replaces `target` with `new`. The running program keeps working until it exits.
fn swap(new: &Path, target: &Path) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("Couldn't replace {}: {e}", target.display());
    if cfg!(all(unix, not(target_os = "macos"))) {
        // A rename over a running executable is fine on Linux.
        return std::fs::rename(new, target).map_err(fail);
    }
    // Windows can't overwrite a running exe but can rename it; a directory can't be renamed
    // over. Either way: park the old one, move the new one in, roll back on failure.
    let old = old_path(target);
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_dir_all(&old);
    std::fs::rename(target, &old).map_err(fail)?;
    if let Err(e) = std::fs::rename(new, target) {
        let _ = std::fs::rename(&old, target);
        return Err(fail(e));
    }
    if target.is_dir() {
        // macOS: the running binary stays mapped, so the old bundle can go now.
        let _ = std::fs::remove_dir_all(&old);
    }
    // Windows: the old exe is still running; cleanup_leftovers deletes it next launch.
    Ok(())
}

/// Starts the freshly installed version. The caller should exit right after.
pub fn relaunch(path: &Path) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg("-n").arg(path);
        c
    } else {
        std::process::Command::new(path)
    };
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Updated, but couldn't restart: {e}. Start Underkeeper again."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.3.0"), Some((0, 3, 0)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(parse_version("nightly"), None);
        assert!(is_newer("v0.3.0", "0.2.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(!is_newer("v0.2.0", "0.2.0"));
        assert!(!is_newer("v0.1.9", "0.2.0"));
        assert!(!is_newer("garbage", "0.2.0"));
    }

    fn sample(tag: &str, digest: Option<&str>) -> serde_json::Value {
        let name = asset_name().unwrap_or("none");
        let mut asset = serde_json::json!({
            "name": name,
            "browser_download_url": format!("https://example.invalid/{name}"),
        });
        if let Some(d) = digest {
            asset["digest"] = d.into();
        }
        serde_json::json!({
            "tag_name": tag,
            "html_url": "https://example.invalid/release",
            "assets": [{"name": "other.zip", "browser_download_url": "x", "digest": "sha256:00"}, asset],
        })
    }

    #[test]
    fn release_parsing() {
        if asset_name().is_none() {
            return;
        }
        let r = parse_release(&sample("v9.0.0", Some("SHA256:ABCD")), "0.2.0")
            .unwrap()
            .unwrap();
        assert_eq!(r.version, "9.0.0");
        assert_eq!(r.sha256, "abcd");
        assert_eq!(r.asset_name, asset_name().unwrap());
        assert!(
            parse_release(&sample("v0.2.0", Some("sha256:ab")), "0.2.0")
                .unwrap()
                .is_none()
        );
        assert!(parse_release(&sample("v9.0.0", None), "0.2.0").is_err());
    }

    /// Real end-to-end run against the live latest release: check, download, verify, swap.
    /// `cargo test -- --ignored live_update`
    #[test]
    #[ignore]
    fn live_update() {
        let rel = check("0.0.1").expect("check").expect("a newer release");
        let dir = std::env::temp_dir().join(format!("underkeeper-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = if cfg!(target_os = "macos") {
            let app = dir.join("Underkeeper.app");
            std::fs::create_dir_all(app.join("Contents")).unwrap();
            app
        } else {
            let exe = dir.join("underkeeper");
            std::fs::write(&exe, b"old").unwrap();
            exe
        };
        let launched = install(&rel, &target).expect("install");
        assert_eq!(launched, target);
        assert_ne!(std::fs::metadata(&target).unwrap().len(), 3);
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(
            leftovers.len(),
            if cfg!(windows) { 2 } else { 1 },
            "{leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
