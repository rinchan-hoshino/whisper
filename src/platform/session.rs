use std::time::{SystemTime, UNIX_EPOCH};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
    REG_VALUE_TYPE,
};

use crate::core::lifecycle::SessionProbe;
use crate::core::protocol::{parse_oopz_login_value, LocalOopzSession};
use crate::platform::to_wide_null;

pub fn oopz_is_running() -> bool {
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) if h != INVALID_HANDLE_VALUE => h,
            _ => return false,
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if name.eq_ignore_ascii_case("oopz.exe") {
                    found = true;
                    break;
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}

pub fn probe_local_oopz_session() -> SessionProbe {
    if !oopz_is_running() {
        return SessionProbe::OopzNotRunning;
    }
    match load_local_oopz_session() {
        Ok(sess) => SessionProbe::Ready(sess),
        Err(err) => SessionProbe::Unavailable(err),
    }
}

pub fn load_local_oopz_session() -> Result<LocalOopzSession, String> {
    if !oopz_is_running() {
        return Err("Oopz 尚未运行，请先打开并登录 Oopz。".to_string());
    }

    let subkey = to_wide_null(r"Software\Oopz\OopzData");
    let value_name = to_wide_null("login");

    unsafe {
        let mut hkey = HKEY::default();
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        );
        if status.is_err() {
            return Err("没有找到已登录的 Oopz，请先打开并登录 Oopz。".to_string());
        }

        let mut val_type = REG_VALUE_TYPE::default();
        let mut byte_len: u32 = 0;
        let query_len = RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut val_type),
            None,
            Some(&mut byte_len),
        );
        if query_len.is_err() || byte_len == 0 || val_type != REG_SZ {
            let _ = RegCloseKey(hkey);
            return Err("没有找到已登录的 Oopz，请先打开并登录 Oopz。".to_string());
        }

        let mut buf: Vec<u16> = vec![0; (byte_len as usize / 2) + 1];
        let query_data = RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut val_type),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut byte_len),
        );
        let _ = RegCloseKey(hkey);
        if query_data.is_err() {
            return Err("无法读取本机 Oopz 登录态。".to_string());
        }

        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let raw_b64 = String::from_utf16_lossy(&buf[..end]);
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        parse_oopz_login_value(&raw_b64, now_secs)
    }
}
