use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::Ime::{
    ImmAssociateContextEx, ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext,
    ImmSetCompositionWindow, CFS_POINT, COMPOSITIONFORM, GCS_COMPSTR, GCS_RESULTSTR, IACE_DEFAULT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, IsIconic, IsWindow, RegisterClassW, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, GWL_EXSTYLE, HWND_TOPMOST,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE,
    SW_SHOWNA, WNDCLASSW, WNDPROC, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::platform::d2d::{DCompTarget, GraphicsEngine};
use crate::platform::hud_render::{render_hud, HudHitMap, HudRenderInput};
use crate::platform::single_instance::HUD_CLASS_NAME;
use crate::platform::to_wide_null;

pub struct HudWindow {
    pub hwnd: HWND,
    pub target: DCompTarget,
    pub hits: HudHitMap,
}

impl HudWindow {
    pub fn create(
        gfx: &GraphicsEngine,
        wndproc: WNDPROC,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<Self, String> {
        let class_w = to_wide_null(HUD_CLASS_NAME);
        let title_w = to_wide_null("Whisper HUD");
        unsafe {
            let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                .map_err(|e| format!("GetModuleHandleW failed: {e}"))?;
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: wndproc,
                hInstance: hinst.into(),
                lpszClassName: PCWSTR(class_w.as_ptr()),
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);

            let ex_style = WS_EX_NOREDIRECTIONBITMAP
                | WS_EX_TOOLWINDOW
                | WS_EX_TOPMOST
                | WS_EX_NOACTIVATE
                | WS_EX_TRANSPARENT;

            let hwnd = CreateWindowExW(
                ex_style,
                PCWSTR(class_w.as_ptr()),
                PCWSTR(title_w.as_ptr()),
                WS_POPUP,
                x,
                y,
                w.max(260),
                h.max(135),
                None,
                None,
                hinst,
                None,
            )
            .map_err(|e| format!("CreateWindowExW HUD failed: {e}"))?;

            let dpi = GetDpiForWindow(hwnd).max(96) as f32;
            let target = DCompTarget::new(gfx, hwnd, w.max(260) as u32, h.max(135) as u32, dpi)?;
            let _ = ShowWindow(hwnd, SW_SHOWNA);
            Ok(Self {
                hwnd,
                target,
                hits: HudHitMap::default(),
            })
        }
    }

    pub fn dpi(&self) -> f32 {
        unsafe { GetDpiForWindow(self.hwnd).max(96) as f32 }
    }

    pub fn set_visible(&self, visible: bool) {
        unsafe {
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNA } else { SW_HIDE });
        }
    }

    pub fn set_click_through(&self, click_through: bool) {
        unsafe {
            let mut ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
            let passive_bits = (WS_EX_TRANSPARENT | WS_EX_NOACTIVATE).0;
            if click_through {
                ex |= passive_bits;
            } else {
                ex &= !passive_bits;
            }
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex as isize);
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }

    pub fn set_bounds(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let w = w.max(260);
        let h = h.max(135);
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
        let dpi = self.dpi();
        let _ = self.target.resize(w as u32, h as u32, dpi);
    }

    pub fn current_rect(&self) -> RECT {
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut r);
        }
        r
    }

    pub fn focus_for_input(&self) {
        self.set_click_through(false);
        unsafe {
            let fg = GetForegroundWindow();
            let cur_tid = GetCurrentThreadId();
            let fg_tid = if !fg.0.is_null() {
                GetWindowThreadProcessId(fg, None)
            } else {
                0
            };
            let attached = fg_tid != 0 && fg_tid != cur_tid && AttachThreadInput(cur_tid, fg_tid, true).as_bool();
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOOWNERZORDER,
            );
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(self.hwnd);
            if attached {
                let _ = AttachThreadInput(cur_tid, fg_tid, false);
            }
            let _ = ImmAssociateContextEx(self.hwnd, None, IACE_DEFAULT);
        }
    }

    pub fn update_ime_position(&self) {
        let scale = self.dpi() / 96.0;
        let px = (self.hits.caret_pos.0 * scale).round() as i32;
        let py = (self.hits.caret_pos.1 * scale).round() as i32;
        unsafe {
            let himc = ImmGetContext(self.hwnd);
            if !himc.0.is_null() {
                let form = COMPOSITIONFORM {
                    dwStyle: CFS_POINT,
                    ptCurrentPos: POINT { x: px, y: py + 4 },
                    rcArea: RECT::default(),
                };
                let _ = ImmSetCompositionWindow(himc, &form);
                let _ = ImmReleaseContext(self.hwnd, himc);
            }
        }
    }

    pub fn read_ime_string(&self, lparam: LPARAM, is_result: bool) -> Option<String> {
        let flag = if is_result { GCS_RESULTSTR } else { GCS_COMPSTR };
        if (lparam.0 as u32 & flag.0) == 0 {
            return None;
        }
        unsafe {
            let himc = ImmGetContext(self.hwnd);
            if himc.0.is_null() {
                return None;
            }
            let bytes = ImmGetCompositionStringW(himc, flag, None, 0);
            let out = if bytes > 0 {
                let mut buf = vec![0u16; (bytes as usize) / 2];
                ImmGetCompositionStringW(
                    himc,
                    flag,
                    Some(buf.as_mut_ptr() as *mut _),
                    bytes as u32,
                );
                Some(String::from_utf16_lossy(&buf))
            } else {
                Some(String::new())
            };
            let _ = ImmReleaseContext(self.hwnd, himc);
            out
        }
    }

    pub fn render(&mut self, gfx: &GraphicsEngine, input: &HudRenderInput<'_>) {
        let (w, h) = self.target.dip_size();
        let mut adjusted = HudRenderInput {
            bounds: D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w,
                bottom: h,
            },
            settings: input.settings,
            messages: input.messages,
            opacities: input.opacities,
            active: input.active,
            adjust_mode: input.adjust_mode,
            connected: input.connected,
            channel_label: input.channel_label,
            channels: input.channels,
            channel_menu_open: input.channel_menu_open,
            hovered_channel_idx: input.hovered_channel_idx,
            draft: input.draft,
            ime_composition: input.ime_composition,
            caret_visible: input.caret_visible,
            scroll_offset_px: input.scroll_offset_px,
            unread_count: input.unread_count,
            copied_toast: input.copied_toast,
            notice_text: input.notice_text,
        };
        if input.bounds.right > 0.0 {
            adjusted.bounds = input.bounds;
        }
        self.target.begin_frame();
        self.hits = render_hud(&self.target.dc, gfx, &adjusted);
        self.target.end_frame(gfx);
    }
}

pub fn capture_foreground_window() -> HWND {
    unsafe { GetForegroundWindow() }
}

pub fn restore_foreground_window(target: HWND, own_hwnd: HWND) {
    unsafe {
        if target.0.is_null()
            || target == own_hwnd
            || !IsWindow(target).as_bool()
            || IsIconic(target).as_bool()
        {
            return;
        }
        let cur_tid = GetCurrentThreadId();
        let target_tid = GetWindowThreadProcessId(target, None);
        let attached = target_tid != 0
            && target_tid != cur_tid
            && AttachThreadInput(cur_tid, target_tid, true).as_bool();
        let _ = SetForegroundWindow(target);
        if attached {
            let _ = AttachThreadInput(cur_tid, target_tid, false);
        }
    }
}

pub fn copy_text_to_clipboard(hwnd: HWND, text: &str) -> bool {
    const CF_UNICODETEXT: u32 = 13;
    let wide = to_wide_null(text);
    let byte_len = wide.len() * 2;
    unsafe {
        if OpenClipboard(hwnd).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, byte_len) else {
            let _ = CloseClipboard();
            return false;
        };
        let ptr = GlobalLock(hmem) as *mut u16;
        if !ptr.is_null() {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(hmem);
            let ok = SetClipboardData(CF_UNICODETEXT, windows::Win32::Foundation::HANDLE(hmem.0)).is_ok();
            let _ = CloseClipboard();
            return ok;
        }
        let _ = CloseClipboard();
        false
    }
}

pub fn point_in_rect(x: f32, y: f32, r: D2D_RECT_F) -> bool {
    x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
}

#[allow(dead_code)]
fn _keep_bool(_: BOOL) {}
