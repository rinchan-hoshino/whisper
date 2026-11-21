use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

use crate::platform::to_wide_null;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Whisper";
const LEGACY_VALUE_NAMES: [&str; 5] = [
    "OopzTacticalOverlay",
    "OopzOverlay",
    "TypeSlip",
    "PaperNote",
    "纸条",
];

fn query_run_value(hkey: HKEY, name: &str) -> Option<String> {
    let val = to_wide_null(name);
    unsafe {
        let mut len = 0u32;
        if RegQueryValueExW(hkey, PCWSTR(val.as_ptr()), None, None, None, Some(&mut len)).is_err()
            || len < 2
        {
            return None;
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2)];
        if RegQueryValueExW(
            hkey,
            PCWSTR(val.as_ptr()),
            None,
            None,
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut len),
        )
        .is_err()
        {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

fn write_run_value(hkey: HKEY, exe_path: &Path) -> Result<(), String> {
    let val = to_wide_null(VALUE_NAME);
    let cmd = format!("\"{}\"", exe_path.to_string_lossy());
    let wide = to_wide_null(&cmd);
    unsafe {
        let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
        RegSetValueExW(hkey, PCWSTR(val.as_ptr()), 0, REG_SZ, Some(bytes))
            .ok()
            .map_err(|e| format!("set Run value failed: {e}"))
    }
}

pub fn is_autostart_enabled() -> bool {
    let subkey = to_wide_null(RUN_KEY_PATH);
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(subkey.as_ptr()), 0, KEY_READ, &mut hkey)
            .is_err()
        {
            return false;
        }
        let ok = query_run_value(hkey, VALUE_NAME)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        let _ = RegCloseKey(hkey);
        ok
    }
}

pub fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
    let subkey = to_wide_null(RUN_KEY_PATH);
    let val = to_wide_null(VALUE_NAME);
    unsafe {
        let mut hkey = HKEY::default();
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            0,
            KEY_WRITE,
            &mut hkey,
        )
        .ok()
        .map_err(|e| format!("open Run registry key failed: {e}"))?;

        let result = if enabled {
            let exe = std::env::current_exe()
                .map_err(|e| format!("current_exe failed: {e}"))?;
            write_run_value(hkey, &exe)
        } else {
            let _ = RegDeleteValueW(hkey, PCWSTR(val.as_ptr()));
            for legacy in LEGACY_VALUE_NAMES {
                let lw = to_wide_null(legacy);
                let _ = RegDeleteValueW(hkey, PCWSTR(lw.as_ptr()));
            }
            Ok(())
        };
        let _ = RegCloseKey(hkey);
        result
    }
}

/// Migrate legacy Run registry keys (`OopzTacticalOverlay`, `OopzOverlay`, `TypeSlip`, `PaperNote`, `纸条`)
/// or stale `"Whisper"` entries pointing to `old_exe` to `"Whisper" = "\"<new_exe>\""`.
pub fn migrate_autostart_entries(old_exe: Option<&Path>, new_exe: &Path) {
    let subkey = to_wide_null(RUN_KEY_PATH);
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            0,
            KEY_READ | KEY_WRITE,
            &mut hkey,
        )
        .is_err()
        {
            return;
        }

        let mut should_write_new = false;
        for legacy in LEGACY_VALUE_NAMES {
            if query_run_value(hkey, legacy).is_some() {
                let lw = to_wide_null(legacy);
                let _ = RegDeleteValueW(hkey, PCWSTR(lw.as_ptr()));
                should_write_new = true;
            }
        }

        if let Some(old) = old_exe {
            let old_norm = old.to_string_lossy().to_ascii_lowercase();
            if let Some(cur_val) = query_run_value(hkey, VALUE_NAME) {
                let cur_norm = cur_val.trim().trim_matches('"').to_ascii_lowercase();
                if cur_norm == old_norm {
                    should_write_new = true;
                }
            }
        }

        if should_write_new {
            let _ = write_run_value(hkey, new_exe);
        }

        let _ = RegCloseKey(hkey);
    }
}
