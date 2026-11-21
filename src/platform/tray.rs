use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, LoadIconW,
    SetForegroundWindow, TrackPopupMenu, HICON, IDI_APPLICATION, MF_SEPARATOR, MF_STRING,
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, WM_APP,
};

use crate::platform::to_wide_null;

pub const WM_APP_TRAY: u32 = WM_APP + 103;
pub const CMD_TRAY_SETTINGS: usize = 4001;
pub const CMD_TRAY_TOGGLE_HUD: usize = 4002;
pub const CMD_TRAY_CHECK_UPDATE: usize = 4003;
pub const CMD_TRAY_QUIT: usize = 4004;

pub struct TrayIcon {
    hwnd: HWND,
    icon: HICON,
    added: bool,
}

impl TrayIcon {
    pub fn new(hwnd: HWND) -> Self {
        let icon = unsafe {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            LoadIconW(hinst, PCWSTR(1usize as *const u16))
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))
                .unwrap_or_default()
        };

        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_APP_TRAY,
            hIcon: icon,
            ..Default::default()
        };
        let tip = to_wide_null("Whisper · Oopz 战术浮层");
        let n = tip.len().min(nid.szTip.len());
        nid.szTip[..n].copy_from_slice(&tip[..n]);

        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &nid).as_bool() };
        Self { hwnd, icon, added }
    }

    pub fn show_context_menu(&self, hud_visible: bool) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            let s_settings = to_wide_null("Whisper 设置…");
            let s_toggle = to_wide_null(if hud_visible {
                "隐藏浮层"
            } else {
                "显示浮层"
            });
            let s_update = to_wide_null("检查更新");
            let s_quit = to_wide_null("退出 Whisper");

            let _ = AppendMenuW(menu, MF_STRING, CMD_TRAY_SETTINGS, PCWSTR(s_settings.as_ptr()));
            let _ = AppendMenuW(menu, MF_STRING, CMD_TRAY_TOGGLE_HUD, PCWSTR(s_toggle.as_ptr()));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, CMD_TRAY_CHECK_UPDATE, PCWSTR(s_update.as_ptr()));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, CMD_TRAY_QUIT, PCWSTR(s_quit.as_ptr()));

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                pt.x,
                pt.y,
                0,
                self.hwnd,
                None,
            );
            let _ = DestroyMenu(menu);
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            if self.added {
                let nid = NOTIFYICONDATAW {
                    cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                    hWnd: self.hwnd,
                    uID: 1,
                    ..Default::default()
                };
                let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            }
            if !self.icon.0.is_null() {
                let _ = DestroyIcon(self.icon);
            }
        }
    }
}
