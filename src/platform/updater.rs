use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use windows::core::{Interface, PCWSTR, PWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READWRITE,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_Startup, IShellLinkW, SHChangeNotify,
    SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT, SHCNE_ASSOCCHANGED, SHCNF_IDLIST,
    SLGP_RAWPATH,
};

use crate::core::settings::APP_VERSION;
use crate::core::updater_core::{
    is_legacy_app_stem, is_newer_version, parse_manifest, verify_bytes, UpdateManifest,
    DEFAULT_MANIFEST_URL,
};
use crate::platform::autostart::migrate_autostart_entries;
use crate::platform::to_wide_null;
use crate::platform::winhttp::http_request;

#[derive(Debug, Clone)]
pub enum UpdateCheckResult {
    UpToDate(String),
    UpdatedAndRestarting(String, PathBuf, Option<PathBuf>),
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

fn target_whisper_path(exe: &Path) -> (PathBuf, bool) {
    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if is_legacy_app_stem(stem) {
        (exe.with_file_name("Whisper.exe"), true)
    } else {
        (exe.to_path_buf(), false)
    }
}

/// On startup, if the running binary has a legacy name (`OopzOverlay.exe`, `TypeSlip.exe`, `纸条.exe`, etc.),
/// migrate it in-place to `Whisper.exe`, update autostart Run keys & `.lnk` shortcuts,
/// and return `Some((new_whisper_exe, old_exe_backup))` so `main()` can hand off and exit cleanly.
pub fn migrate_legacy_binary_on_startup() -> Option<(PathBuf, PathBuf)> {
    let exe = std::env::current_exe().ok()?;
    let (new_exe, is_legacy) = target_whisper_path(&exe);
    if !is_legacy {
        // Still clean up any stale legacy autostart keys pointing to old names
        migrate_autostart_entries(None, &exe);
        return None;
    }

    let temp_new = PathBuf::from(format!("{}.new", new_exe.display()));
    let old_backup = PathBuf::from(format!("{}.old", exe.display()));

    let _ = fs::remove_file(&temp_new);
    let _ = fs::remove_file(&old_backup);

    if fs::copy(&exe, &temp_new).is_err() {
        let _ = fs::remove_file(&temp_new);
        return None;
    }
    if fs::rename(&exe, &old_backup).is_err() {
        let _ = fs::remove_file(&temp_new);
        return None;
    }
    if new_exe.exists() {
        let _ = fs::remove_file(&new_exe);
    }
    if fs::rename(&temp_new, &new_exe).is_err() {
        let _ = fs::rename(&old_backup, &exe);
        let _ = fs::remove_file(&temp_new);
        return None;
    }

    migrate_autostart_entries(Some(&exe), &new_exe);
    migrate_shortcuts(&exe, &new_exe);
    Some((new_exe, old_backup))
}

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let raw: PWSTR = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const _));
        s.map(PathBuf::from)
    }
}

fn collect_lnk_files(dir: &Path, max_depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("lnk"))
                .unwrap_or(false)
            {
                out.push(path);
            }
        } else if path.is_dir() && max_depth > 0 {
            collect_lnk_files(&path, max_depth - 1, out);
        }
    }
}

fn normalize_win_path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\");
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_ascii_lowercase()
}

/// Scan Desktop, Start Menu Programs, and Startup for `.lnk` shortcuts pointing to `old_exe`,
/// retarget them to `new_exe`, clear custom icon override, rename legacy `.lnk` names to `Whisper.lnk`,
/// and notify Explorer to refresh the icon cache.
pub fn migrate_shortcuts(old_exe: &Path, new_exe: &Path) {
    let old_norm = normalize_win_path(old_exe);
    let mut roots: Vec<(PathBuf, usize)> = Vec::new();
    if let Some(p) = known_folder(&FOLDERID_Desktop) {
        roots.push((p, 0));
    } else if let Ok(profile) = std::env::var("USERPROFILE") {
        roots.push((PathBuf::from(profile).join("Desktop"), 0));
    }
    if let Some(p) = known_folder(&FOLDERID_Programs) {
        roots.push((p, 2));
    } else if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push((
            PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs"),
            2,
        ));
    }
    if let Some(p) = known_folder(&FOLDERID_Startup) {
        roots.push((p, 0));
    } else if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push((
            PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup"),
            0,
        ));
    }

    let mut lnks = Vec::new();
    for (root, depth) in roots {
        collect_lnk_files(&root, depth, &mut lnks);
    }
    if lnks.is_empty() {
        return;
    }

    unsafe {
        let com_inited = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let mut any_changed = false;
        let new_exe_w = to_wide_null(&new_exe.to_string_lossy());
        let new_dir_w = new_exe
            .parent()
            .map(|p| to_wide_null(&p.to_string_lossy()));
        let empty_w = to_wide_null("");

        for lnk in lnks {
            let Ok(shell_link): windows::core::Result<IShellLinkW> =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            else {
                continue;
            };
            let Ok(persist): windows::core::Result<IPersistFile> = shell_link.cast() else {
                continue;
            };
            let lnk_w = to_wide_null(&lnk.to_string_lossy());
            if persist.Load(PCWSTR(lnk_w.as_ptr()), STGM_READWRITE).is_err() {
                continue;
            }

            let mut target_buf = [0u16; 1024];
            if shell_link
                .GetPath(&mut target_buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
                .is_err()
            {
                continue;
            }
            let end = target_buf
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(target_buf.len());
            let target_str = String::from_utf16_lossy(&target_buf[..end]);
            if normalize_win_path(Path::new(&target_str)) != old_norm {
                continue;
            }

            let _ = shell_link.SetPath(PCWSTR(new_exe_w.as_ptr()));
            if let Some(ref dir_w) = new_dir_w {
                let _ = shell_link.SetWorkingDirectory(PCWSTR(dir_w.as_ptr()));
            }
            let _ = shell_link.SetIconLocation(PCWSTR(empty_w.as_ptr()), 0);
            if persist.Save(PCWSTR(lnk_w.as_ptr()), true).is_ok() {
                any_changed = true;
            }
            drop(persist);
            drop(shell_link);

            let lnk_stem = lnk.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if is_legacy_app_stem(lnk_stem) {
                let renamed = lnk.with_file_name("Whisper.lnk");
                if renamed != lnk {
                    if renamed.exists() {
                        let _ = fs::remove_file(&renamed);
                    }
                    if fs::rename(&lnk, &renamed).is_ok() {
                        any_changed = true;
                    }
                }
            }
        }

        if any_changed {
            SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
        }
        if com_inited {
            CoUninitialize();
        }
    }
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
        Ok((exe_path, old_path)) => {
            UpdateCheckResult::UpdatedAndRestarting(manifest.version, exe_path, Some(old_path))
        }
        Err(e) => UpdateCheckResult::Error(format!("替换程序失败: {e}")),
    }
}

/// Atomically replace the running `.exe` (migrating legacy filenames to `Whisper.exe`):
/// 1. Write verified new bytes to `<target>.new` on the same volume
/// 2. Rename running `<exe>` -> `<exe>.old` (Windows allows renaming a running image!)
/// 3. Rename `<target>.new` -> `<target>`
/// 4. If migrated from a legacy name, update autostart registry & `.lnk` shortcuts
/// 5. Return `(target, old_path)` so `main()` spawns the new binary with `--cleanup-updater`
fn atomic_self_replace(new_bytes: &[u8]) -> Result<(PathBuf, PathBuf), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let (target_exe, is_legacy) = target_whisper_path(&exe);
    let new_path = PathBuf::from(format!("{}.new", target_exe.display()));
    let old_path = PathBuf::from(format!("{}.old", exe.display()));

    let _ = fs::remove_file(&new_path);
    let _ = fs::remove_file(&old_path);

    fs::write(&new_path, new_bytes).map_err(|e| format!("write new exe: {e}"))?;
    if let Err(e) = fs::rename(&exe, &old_path) {
        let _ = fs::remove_file(&new_path);
        return Err(format!("rename running exe to .old: {e}"));
    }
    if is_legacy && target_exe.exists() {
        let _ = fs::remove_file(&target_exe);
    }
    if let Err(e) = fs::rename(&new_path, &target_exe) {
        let _ = fs::rename(&old_path, &exe);
        let _ = fs::remove_file(&new_path);
        return Err(format!("move new exe into place: {e}"));
    }

    if is_legacy {
        migrate_autostart_entries(Some(&exe), &target_exe);
        migrate_shortcuts(&exe, &target_exe);
    }

    Ok((target_exe, old_path))
}
