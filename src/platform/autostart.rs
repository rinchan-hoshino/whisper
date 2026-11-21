use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

use crate::platform::to_wide_null;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Whisper";

pub fn is_autostart_enabled() -> bool {
    let subkey = to_wide_null(RUN_KEY_PATH);
    let val = to_wide_null(VALUE_NAME);
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(subkey.as_ptr()), 0, KEY_READ, &mut hkey)
            .is_err()
        {
            return false;
        }
        let mut len = 0u32;
        let ok = RegQueryValueExW(hkey, PCWSTR(val.as_ptr()), None, None, None, Some(&mut len))
            .is_ok()
            && len > 2;
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
            let cmd = format!("\"{}\"", exe.to_string_lossy());
            let wide = to_wide_null(&cmd);
            let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
            RegSetValueExW(hkey, PCWSTR(val.as_ptr()), 0, REG_SZ, Some(bytes))
                .ok()
                .map_err(|e| format!("set Run value failed: {e}"))
        } else {
            let _ = RegDeleteValueW(hkey, PCWSTR(val.as_ptr()));
            Ok(())
        };
        let _ = RegCloseKey(hkey);
        result
    }
}
