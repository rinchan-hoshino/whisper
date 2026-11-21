use std::fs;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::LocalFree;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

use crate::core::settings::AppSettings;
use crate::platform::to_wide_null;

fn state_root() -> PathBuf {
    if let Ok(custom) = std::env::var("WHISPER_STATE_ROOT") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("RinChan")
}

pub fn state_dir() -> PathBuf {
    state_root().join("Whisper")
}

pub fn legacy_state_dir() -> PathBuf {
    state_root().join("OopzTacticalOverlay")
}

pub fn settings_path() -> PathBuf {
    state_dir().join("settings.dat")
}

pub fn protect_bytes(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    unsafe {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: plaintext.len() as u32,
            pbData: plaintext.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        let desc = to_wide_null("Whisper");
        CryptProtectData(
            &mut input,
            PCWSTR(desc.as_ptr()),
            None,
            None,
            None,
            0,
            &mut output,
        )
        .map_err(|e| format!("CryptProtectData failed: {e}"))?;

        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let result = slice.to_vec();
        let _ = LocalFree(windows::Win32::Foundation::HLOCAL(output.pbData as _));
        Ok(result)
    }
}

pub fn unprotect_bytes(ciphertext: &[u8]) -> Result<Vec<u8>, String> {
    unsafe {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: ciphertext.len() as u32,
            pbData: ciphertext.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(&mut input, None, None, None, None, 0, &mut output)
            .map_err(|e| format!("CryptUnprotectData failed: {e}"))?;

        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let result = slice.to_vec();
        let _ = LocalFree(windows::Win32::Foundation::HLOCAL(output.pbData as _));
        Ok(result)
    }
}

fn load_dpapi_settings_file(path: &Path) -> Result<AppSettings, String> {
    let raw = fs::read(path).map_err(|e| format!("read {} failed: {e}", path.display()))?;
    // Support both DPAPI-encrypted blobs (production 1.x & 2.0) and plain UTF-8 JSON fallback
    let json_bytes = if raw.first() == Some(&b'{') {
        raw
    } else {
        unprotect_bytes(&raw)?
    };
    AppSettings::from_json_bytes(&json_bytes)
}

/// Loads Whisper settings from `%LOCALAPPDATA%\RinChan\Whisper\settings.dat`.
/// If absent, automatically migrates losslessly from 1.x `%LOCALAPPDATA%\RinChan\OopzTacticalOverlay\state.bin`
/// (or `settings.dat` / `settings.json`).
pub fn load_or_migrate_settings() -> (AppSettings, bool) {
    let target = settings_path();
    if target.is_file() {
        if let Ok(settings) = load_dpapi_settings_file(&target) {
            return (settings, false);
        }
    }

    let legacy_dir = legacy_state_dir();
    for candidate in ["state.bin", "settings.dat", "settings.json"] {
        let legacy_path = legacy_dir.join(candidate);
        if legacy_path.is_file() {
            if let Ok(migrated) = load_dpapi_settings_file(&legacy_path) {
                let _ = save_settings(&migrated);
                return (migrated, true);
            }
        }
    }

    (AppSettings::default(), false)
}

pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let target = settings_path();
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create settings dir failed: {e}"))?;
    }
    let json_bytes = settings.to_json_bytes()?;
    let encrypted = protect_bytes(&json_bytes)?;
    let tmp = target.with_extension("dat.tmp");
    fs::write(&tmp, &encrypted).map_err(|e| format!("write temp settings failed: {e}"))?;
    fs::rename(&tmp, &target).or_else(|_| {
        let _ = fs::remove_file(&target);
        fs::rename(&tmp, &target)
    }).map_err(|e| format!("replace settings failed: {e}"))?;
    Ok(())
}
