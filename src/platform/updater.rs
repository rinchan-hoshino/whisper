use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::core::settings::APP_VERSION;
use crate::core::updater_core::{
    is_newer_version, parse_manifest, verify_bytes, UpdateManifest, DEFAULT_MANIFEST_URL,
};
use crate::platform::winhttp::http_request;

#[derive(Debug, Clone)]
pub enum UpdateCheckResult {
    UpToDate(String),
    UpdatedAndRestarting(String, PathBuf),
    Error(String),
}

pub fn cleanup_old_binaries() {
    if let Ok(exe) = std::env::current_exe() {
        let old = PathBuf::from(format!("{}.old", exe.display()));
        if old.exists() {
            thread::spawn(move || {
                for _ in 0..20 {
                    if fs::remove_file(&old).is_ok() || !old.exists() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(250));
                }
            });
        }
    }
}

pub fn cleanup_explicit_path(path: &Path) {
    let target = path.to_path_buf();
    thread::spawn(move || {
        for _ in 0..20 {
            if fs::remove_file(&target).is_ok() || !target.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
    });
}

fn manifest_url() -> (String, bool) {
    if let Ok(custom) = std::env::var("WHISPER_UPDATE_MANIFEST_URL") {
        let trimmed = custom.trim().to_string();
        if !trimmed.is_empty() {
            let allow_loopback = trimmed.starts_with("http://127.0.0.1:")
                || trimmed.starts_with("http://localhost:");
            return (trimmed, allow_loopback);
        }
    }
    (DEFAULT_MANIFEST_URL.to_string(), false)
}

pub fn check_and_apply_update() -> UpdateCheckResult {
    let (url, allow_loopback) = manifest_url();
    let headers = vec![("User-Agent".to_string(), format!("Whisper/{APP_VERSION}"))];

    let resp = match http_request("GET", &url, &headers, None, 8000) {
        Ok(r) if r.status == 200 => r,
        Ok(r) => return UpdateCheckResult::Error(format!("检查更新失败 (HTTP {})", r.status)),
        Err(e) => return UpdateCheckResult::Error(format!("检查更新失败: {e}")),
    };

    let manifest: UpdateManifest = match parse_manifest(&resp.body, allow_loopback) {
        Ok(m) => m,
        Err(e) => return UpdateCheckResult::Error(format!("更新清单无效: {e}")),
    };

    match is_newer_version(&manifest.version, APP_VERSION) {
        Ok(true) => {}
        Ok(false) => return UpdateCheckResult::UpToDate(APP_VERSION.to_string()),
        Err(e) => return UpdateCheckResult::Error(e),
    }

    let bin_resp = match http_request("GET", &manifest.url, &headers, None, 30_000) {
        Ok(r) if r.status == 200 => r,
        Ok(r) => return UpdateCheckResult::Error(format!("下载更新失败 (HTTP {})", r.status)),
        Err(e) => return UpdateCheckResult::Error(format!("下载更新失败: {e}")),
    };

    if let Err(e) = verify_bytes(&bin_resp.body, &manifest.sha256, manifest.size) {
        return UpdateCheckResult::Error(format!("更新包校验失败: {e}"));
    }

    match atomic_self_replace(&bin_resp.body) {
        Ok(exe_path) => UpdateCheckResult::UpdatedAndRestarting(manifest.version, exe_path),
        Err(e) => UpdateCheckResult::Error(format!("替换程序失败: {e}")),
    }
}

/// Atomically replace the running `.exe`:
/// 1. Write verified new bytes to `<exe>.new` on the same volume
/// 2. Rename running `<exe>` -> `<exe>.old` (Windows allows renaming a running image!)
/// 3. Rename `<exe>.new` -> `<exe>`
/// 4. Return `<exe>` path so `main()` spawns the new binary after dropping `SingleInstanceGuard`
fn atomic_self_replace(new_bytes: &[u8]) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let new_path = PathBuf::from(format!("{}.new", exe.display()));
    let old_path = PathBuf::from(format!("{}.old", exe.display()));

    let _ = fs::remove_file(&new_path);
    let _ = fs::remove_file(&old_path);

    fs::write(&new_path, new_bytes).map_err(|e| format!("write new exe: {e}"))?;
    if let Err(e) = fs::rename(&exe, &old_path) {
        let _ = fs::remove_file(&new_path);
        return Err(format!("rename running exe to .old: {e}"));
    }
    if let Err(e) = fs::rename(&new_path, &exe) {
        let _ = fs::rename(&old_path, &exe);
        let _ = fs::remove_file(&new_path);
        return Err(format!("move new exe into place: {e}"));
    }

    Ok(exe)
}
