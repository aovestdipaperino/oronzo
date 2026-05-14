//! Self-update for the oronzo binary.
//!
//! `oronzo upgrade` downloads the latest release from GitHub, extracts the
//! binary, and replaces the running executable. Install-method-aware: handles
//! Homebrew Cellar symlinks and Scoop version directories.
//!
//! `check_for_update` is called on every command with a very short timeout so
//! it never noticeably delays normal usage.

use std::path::Path;
use std::time::Duration;

const GITHUB_REPO: &str = "aovestdipaperino/oronzo";
const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/aovestdipaperino/oronzo/releases/latest";

// ── Version check (called on every command) ────────────────────────────

/// Non-blocking version check with a very short timeout.
/// Prints a one-liner to stderr if a newer version is available.
pub fn check_for_update() {
    let current = env!("CARGO_PKG_VERSION");
    let Some(latest) = fetch_latest_version() else {
        return;
    };
    if is_newer_version(current, &latest) {
        eprintln!(
            "\x1b[33moronzo v{latest} available\x1b[0m (current: v{current}) — run `oronzo upgrade`"
        );
    }
}

/// Fetches the latest stable release version from GitHub with a short timeout.
fn fetch_latest_version() -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(500)))
        .build()
        .into();

    let release: Release = agent
        .get(GITHUB_RELEASES_URL)
        .header("User-Agent", "oronzo")
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;
    Some(release.tag_name.trim_start_matches('v').to_string())
}

/// Semver comparison. Returns true if `latest` is strictly newer than `current`.
fn is_newer_version(current: &str, latest: &str) -> bool {
    fn parse(v: &str) -> Option<(u64, u64, u64)> {
        let base = v.split_once('-').map_or(v, |(b, _)| b);
        let mut parts = base.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        Some((major, minor, patch))
    }

    match (parse(current), parse(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

// ── Install method detection ───────────────────────────────────────────

enum InstallMethod {
    Cargo,
    Brew,
    Scoop,
    Unknown,
}

fn detect_install_method() -> InstallMethod {
    let Ok(exe) = std::env::current_exe() else {
        return InstallMethod::Unknown;
    };
    let path = exe.to_string_lossy();
    if path.contains(".cargo/bin") || path.contains(".cargo\\bin") {
        InstallMethod::Cargo
    } else if path.contains("/homebrew/") || path.contains("/Cellar/") {
        InstallMethod::Brew
    } else if path.contains("\\scoop\\") || path.contains("/scoop/") {
        InstallMethod::Scoop
    } else {
        InstallMethod::Unknown
    }
}

// ── Upgrade command ────────────────────────────────────────────────────

fn asset_name(version: &str) -> String {
    let platform = current_platform();
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    format!("oronzo-v{version}-{platform}.{ext}")
}

fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "aarch64-macos"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        "x86_64-macos"
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        "x86_64-linux"
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
        "aarch64-linux"
    } else if cfg!(target_os = "windows") {
        "x86_64-windows"
    } else {
        "unknown"
    }
}

fn fetch_asset_url(tag: &str, expected_asset: &str) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
    }
    #[derive(serde::Deserialize)]
    struct Release {
        assets: Vec<Asset>,
    }

    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/tags/{tag}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();

    let release: Release = agent
        .get(&url)
        .header("User-Agent", "oronzo")
        .call()
        .map_err(|e| format!("failed to reach GitHub: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("failed to parse release info: {e}"))?;

    release
        .assets
        .into_iter()
        .find(|a| a.name == expected_asset)
        .map(|a| a.browser_download_url)
        .ok_or_else(|| {
            format!(
                "release {tag} exists but asset '{expected_asset}' is not yet available.\n  \
                 CI build may still be in progress — try again in a few minutes.\n  \
                 https://github.com/{GITHUB_REPO}/releases/tag/{tag}"
            )
        })
}

fn download_and_extract(url: &str, bin_name: &str) -> Result<std::path::PathBuf, String> {
    let tmp_path = std::env::temp_dir().join(format!(
        "oronzo_upgrade_{}{}",
        std::process::id(),
        if cfg!(windows) { ".exe" } else { "" }
    ));

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_mins(5)))
        .build()
        .into();

    eprint!("  Downloading...");

    let raw: Vec<u8> = {
        use std::io::Read;
        let mut buf = Vec::new();
        agent
            .get(url)
            .header("User-Agent", "oronzo")
            .call()
            .map_err(|e| format!("download failed: {e}"))?
            .body_mut()
            .as_reader()
            .read_to_end(&mut buf)
            .map_err(|e| format!("download read failed: {e}"))?;
        buf
    };

    eprintln!(" ({:.1} MiB)", raw.len() as f64 / 1_048_576.0);
    eprint!("  Extracting...");

    #[cfg(not(windows))]
    extract_targz(&raw, bin_name, &tmp_path)?;

    #[cfg(windows)]
    extract_zip(&raw, bin_name, &tmp_path)?;

    eprintln!(" Done");
    Ok(tmp_path)
}

#[cfg(not(windows))]
fn extract_targz(data: &[u8], bin_name: &str, dest: &Path) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use std::io::Cursor;
    use tar::Archive;

    let gz = GzDecoder::new(Cursor::new(data));
    let mut archive = Archive::new(gz);

    for entry in archive
        .entries()
        .map_err(|e| format!("archive open failed: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("archive read failed: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("archive path error: {e}"))?
            .to_path_buf();

        if path.file_name().and_then(|n| n.to_str()) == Some(bin_name) {
            entry
                .unpack(dest)
                .map_err(|e| format!("extract failed: {e}"))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(dest)
                    .map_err(|e| format!("stat failed: {e}"))?
                    .permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(dest, perms).map_err(|e| format!("chmod failed: {e}"))?;
            }

            return Ok(());
        }
    }

    Err(format!("binary '{bin_name}' not found in archive"))
}

#[cfg(windows)]
fn extract_zip(data: &[u8], bin_name: &str, dest: &Path) -> Result<(), String> {
    use std::io::Cursor;

    let mut archive =
        zip::ZipArchive::new(Cursor::new(data)).map_err(|e| format!("zip open failed: {e}"))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("zip entry error: {e}"))?;

        if Path::new(file.name()).file_name().and_then(|n| n.to_str()) == Some(bin_name) {
            let mut out =
                std::fs::File::create(dest).map_err(|e| format!("create temp file failed: {e}"))?;
            std::io::copy(&mut file, &mut out).map_err(|e| format!("extract failed: {e}"))?;
            return Ok(());
        }
    }

    Err(format!("binary '{bin_name}' not found in zip"))
}

// ── Binary replacement ─────────────────────────────────────────────────

fn replace_binary(new_exe: &Path, method: &InstallMethod, new_version: &str) -> Result<(), String> {
    let result = match method {
        InstallMethod::Brew => replace_for_brew(new_exe, new_version),
        InstallMethod::Scoop => replace_for_scoop(new_exe, new_version),
        _ => replace_default(new_exe),
    };
    let _ = std::fs::remove_file(new_exe);
    result
}

fn replace_default(new_exe: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        let exe = std::env::current_exe().ok();
        let canonical = exe.as_ref().and_then(|e| e.canonicalize().ok());
        if let (Some(exe), Some(ref canonical)) = (&exe, canonical) {
            if exe.as_path() != canonical.as_path() {
                return install_binary(new_exe, canonical);
            }
        }
    }

    self_replace::self_replace(new_exe).map_err(|e| {
        format!(
            "binary replacement failed: {e}\n  \
             The old version is still in place.\n  \
             To upgrade manually: https://github.com/{GITHUB_REPO}/releases/latest"
        )
    })
}

#[cfg(unix)]
fn install_binary(src: &Path, target: &Path) -> Result<(), String> {
    let dir = target
        .parent()
        .ok_or_else(|| "cannot determine target directory".to_string())?;
    let temp = dir.join(format!(".oronzo_upgrade_{}", std::process::id()));

    std::fs::copy(src, &temp).map_err(|e| format!("cannot copy new binary: {e}"))?;

    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("cannot set permissions: {e}"))?;
    }

    if let Err(e) = std::fs::rename(&temp, target) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("cannot replace binary: {e}"));
    }

    Ok(())
}

// ── Homebrew ───────────────────────────────────────────────────────────

#[cfg(unix)]
fn replace_for_brew(new_exe: &Path, new_version: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot determine current exe: {e}"))?;
    let canonical = exe
        .canonicalize()
        .map_err(|e| format!("cannot resolve binary path: {e}"))?;

    let bin_dir = match canonical.parent() {
        Some(p) if p.file_name().and_then(|n| n.to_str()) == Some("bin") => p,
        _ => return replace_default(new_exe),
    };
    let Some(version_dir) = bin_dir.parent() else {
        return replace_default(new_exe);
    };
    let Some(formula_dir) = version_dir.parent() else {
        return replace_default(new_exe);
    };
    let cellar_dir = match formula_dir.parent() {
        Some(p) if p.file_name().and_then(|n| n.to_str()) == Some("Cellar") => p,
        _ => return replace_default(new_exe),
    };
    let Some(prefix) = cellar_dir.parent() else {
        return replace_default(new_exe);
    };

    let Some(bin_name) = canonical.file_name() else {
        return replace_default(new_exe);
    };
    let Some(old_version_os) = version_dir.file_name() else {
        return replace_default(new_exe);
    };
    let old_version = old_version_os.to_string_lossy().to_string();

    install_binary(new_exe, &canonical)?;

    if old_version != new_version {
        let new_version_dir = formula_dir.join(new_version);

        match std::fs::rename(version_dir, &new_version_dir) {
            Ok(()) => {
                let symlink_path = prefix.join("bin").join(bin_name);
                if let Ok(meta) = std::fs::symlink_metadata(&symlink_path) {
                    if meta.file_type().is_symlink() {
                        if let Ok(old_target) = std::fs::read_link(&symlink_path) {
                            let new_target = std::path::PathBuf::from(
                                old_target
                                    .to_string_lossy()
                                    .replacen(&old_version, new_version, 1),
                            );
                            let _ = std::fs::remove_file(&symlink_path);
                            let _ = std::os::unix::fs::symlink(&new_target, &symlink_path);
                        }
                    }
                }

                let receipt = new_version_dir.join("INSTALL_RECEIPT.json");
                if receipt.exists() {
                    if let Ok(text) = std::fs::read_to_string(&receipt) {
                        let _ = std::fs::write(&receipt, text.replace(&old_version, new_version));
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "\n  \x1b[33mwarning:\x1b[0m could not rename Cellar directory: {e}\n    \
                     brew may still report the old version"
                );
            }
        }
    }

    Ok(())
}

#[cfg(not(unix))]
fn replace_for_brew(new_exe: &Path, _new_version: &str) -> Result<(), String> {
    replace_default(new_exe)
}

// ── Scoop ──────────────────────────────────────────────────────────────

#[cfg(windows)]
fn replace_for_scoop(new_exe: &Path, new_version: &str) -> Result<(), String> {
    self_replace::self_replace(new_exe).map_err(|e| {
        format!(
            "binary replacement failed: {e}\n  \
             The old version is still in place.\n  \
             To upgrade manually: https://github.com/{GITHUB_REPO}/releases/latest"
        )
    })?;

    update_scoop_metadata(new_version);
    Ok(())
}

#[cfg(windows)]
fn update_scoop_metadata(new_version: &str) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let canonical = exe.canonicalize().unwrap_or(exe);

    let Some(version_dir) = find_scoop_version_dir(&canonical) else {
        return;
    };
    let Some(app_dir) = version_dir.parent() else {
        return;
    };
    let old_version = version_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    if old_version == new_version || old_version == "current" {
        return;
    }

    let new_version_dir = app_dir.join(new_version);
    if std::fs::create_dir_all(&new_version_dir).is_err() {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(&version_dir) {
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name();
            if name.to_string_lossy().contains("__self_delete__") {
                continue;
            }
            let _ = std::fs::copy(entry.path(), new_version_dir.join(&name));
        }
    }

    let manifest = new_version_dir.join("manifest.json");
    if manifest.exists() {
        if let Ok(text) = std::fs::read_to_string(&manifest) {
            let _ = std::fs::write(&manifest, text.replace(&old_version, new_version));
        }
    }

    let current = app_dir.join("current");
    let _ = std::fs::remove_dir(&current);
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("cmd")
        .args([
            "/c",
            "mklink",
            "/J",
            &current.to_string_lossy(),
            &new_version_dir.to_string_lossy(),
        ])
        .creation_flags(0x08000000)
        .status();
}

#[cfg(windows)]
fn find_scoop_version_dir(path: &Path) -> Option<std::path::PathBuf> {
    let mut found_apps = false;
    let mut depth_after_apps = 0u8;
    let mut result = std::path::PathBuf::new();

    for comp in path.components() {
        result.push(comp);
        if found_apps {
            depth_after_apps += 1;
            if depth_after_apps == 2 {
                return Some(result);
            }
        } else if let std::path::Component::Normal(name) = comp {
            if name.to_string_lossy().eq_ignore_ascii_case("apps") {
                found_apps = true;
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn replace_for_scoop(new_exe: &Path, _new_version: &str) -> Result<(), String> {
    replace_default(new_exe)
}

// ── Public entry point ─────────────────────────────────────────────────

pub fn run() {
    let current = env!("CARGO_PKG_VERSION");
    let method = detect_install_method();

    let method_suffix = match &method {
        InstallMethod::Brew => " (Homebrew)",
        InstallMethod::Scoop => " (Scoop)",
        InstallMethod::Cargo => " (cargo)",
        InstallMethod::Unknown => "",
    };
    eprintln!("Current version: v{current}{method_suffix}");
    eprintln!("Checking for updates...");

    let Some(latest) = fetch_latest_version() else {
        eprintln!("Failed to check for updates — could not reach GitHub.");
        std::process::exit(1);
    };

    if !is_newer_version(current, &latest) {
        eprintln!("\x1b[32m✔\x1b[0m Already up to date (v{current}).");
        return;
    }

    eprintln!("Upgrading v{current} → v{latest}...");

    let tag = format!("v{latest}");
    let expected = asset_name(&latest);
    eprintln!("  Asset: {expected}");

    let asset_url = match fetch_asset_url(&tag, &expected) {
        Ok(url) => url,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let bin_name = if cfg!(windows) {
        "oronzo.exe"
    } else {
        "oronzo"
    };

    let tmp = match download_and_extract(&asset_url, bin_name) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let label = match method {
        InstallMethod::Brew => " (Homebrew Cellar)",
        InstallMethod::Scoop => " (Scoop)",
        _ => "",
    };
    eprint!("  Replacing binary{label}...");

    if let Err(e) = replace_binary(&tmp, &method, &latest) {
        eprintln!("\nError: {e}");
        std::process::exit(1);
    }

    eprintln!(" Done");
    eprintln!("\x1b[32m✔\x1b[0m Successfully upgraded to v{latest}!");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_version_detected() {
        assert!(is_newer_version("0.3.0", "0.4.0"));
        assert!(is_newer_version("0.3.0", "1.0.0"));
        assert!(is_newer_version("0.3.0", "0.3.1"));
    }

    #[test]
    fn same_version_not_newer() {
        assert!(!is_newer_version("0.3.0", "0.3.0"));
    }

    #[test]
    fn older_version_not_newer() {
        assert!(!is_newer_version("1.0.0", "0.9.0"));
    }

    #[test]
    fn current_platform_not_unknown() {
        assert_ne!(current_platform(), "unknown");
    }

    #[test]
    fn asset_name_matches_ci_convention() {
        let name = asset_name("0.4.0");
        let platform = current_platform();
        if cfg!(windows) {
            assert_eq!(name, format!("oronzo-v0.4.0-{platform}.zip"));
        } else {
            assert_eq!(name, format!("oronzo-v0.4.0-{platform}.tar.gz"));
        }
    }
}
