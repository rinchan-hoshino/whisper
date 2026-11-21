use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, LPARAM, WPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_APP};

use crate::platform::to_wide_null;

pub const HUD_CLASS_NAME: &str = "RinChan.Whisper.HudWindow";
pub const SETTINGS_CLASS_NAME: &str = "RinChan.Whisper.SettingsWindow";
pub const WM_APP_SHOW_SETTINGS: u32 = WM_APP + 101;

pub struct SingleInstanceGuard {
    handle: HANDLE,
}

impl SingleInstanceGuard {
    pub fn acquire() -> Option<Self> {
        let state_dir = crate::platform::dpapi::state_dir();
        let hash = crate::core::protocol::OopzSigner::body_md5(
            "whisper-instance",
            &state_dir.to_string_lossy(),
        );
        let mutex_name = to_wide_null(&format!("Local\\RinChan_Whisper_{}", &hash[..16]));
        unsafe {
            let handle = CreateMutexW(None, false, PCWSTR(mutex_name.as_ptr())).ok()?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                notify_existing_instance();
                return None;
            }
            Some(Self { handle })
        }
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

fn notify_existing_instance() {
    let class = to_wide_null(HUD_CLASS_NAME);
    unsafe {
        if let Ok(hwnd) = FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) {
            if !hwnd.0.is_null() {
                let _ = PostMessageW(hwnd, WM_APP_SHOW_SETTINGS, WPARAM(0), LPARAM(0));
            }
        }
    }
}
