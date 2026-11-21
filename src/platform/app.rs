use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, ReleaseCapture, SetCapture, UnregisterHotKey, VK_BACK,
    VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, KillTimer,
    PostMessageW, PostQuitMessage, SendMessageW, SetTimer, ShowWindow,
    TranslateMessage, HTCAPTION, MSG, SM_CXSCREEN, SM_CYSCREEN, SW_MINIMIZE, WM_APP, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_HOTKEY,
    WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_STARTCOMPOSITION, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCLBUTTONDOWN, WM_RBUTTONUP, WM_TIMER,
};

use crate::core::hotkey::HotkeySpec;
use crate::core::protocol::{AreaInfo, LocalOopzSession, TextChannel};
use crate::core::settings::{AppSettings, DockCorner};
use crate::core::timeline::Timeline;
use crate::platform::autostart::{is_autostart_enabled, set_autostart_enabled};
use crate::platform::d2d::GraphicsEngine;
use crate::platform::dpapi::{load_or_migrate_settings, save_settings};
use crate::platform::gateway::{GatewayEvent, GatewayHandle, WM_APP_GATEWAY_EVENT};
use crate::platform::hud_render::{sample_preview_messages, HudRenderInput};
use crate::platform::hud_window::{
    capture_foreground_window, copy_text_to_clipboard, point_in_rect, restore_foreground_window,
    HudWindow,
};
use crate::platform::settings_window::{
    ActiveSlider, OpenDropdown, RecordingHotkey, SettingsViewState, SettingsWindow,
};
use crate::platform::single_instance::WM_APP_SHOW_SETTINGS;
use crate::platform::sound::{enumerate_sounds, play_sound, SoundOption};
use crate::platform::tray::{
    TrayIcon, CMD_TRAY_CHECK_UPDATE, CMD_TRAY_QUIT, CMD_TRAY_SETTINGS, CMD_TRAY_TOGGLE_HUD,
    WM_APP_TRAY,
};
use crate::platform::updater::{check_and_apply_update, UpdateCheckResult};

const HOTKEY_ACTIVATE_ID: i32 = 0xB001;
const HOTKEY_VISIBILITY_ID: i32 = 0xB002;

const TIMER_FADE: usize = 1;
const TIMER_CARET: usize = 2;
const TIMER_TOAST: usize = 3;
const TIMER_SESSION_POLL: usize = 4;

const WM_APP_UPDATE_RESULT: u32 = WM_APP + 104;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragKind {
    None,
    Move,
    Resize,
}

struct AppState {
    gfx: GraphicsEngine,
    hud: HudWindow,
    settings_win: SettingsWindow,
    tray: TrayIcon,
    gateway: GatewayHandle,
    gateway_rx: mpsc::Receiver<GatewayEvent>,
    update_rx: mpsc::Receiver<UpdateCheckResult>,
    update_tx: mpsc::Sender<UpdateCheckResult>,

    settings: AppSettings,
    session: Option<LocalOopzSession>,
    areas: Vec<AreaInfo>,
    channels: Vec<TextChannel>,
    sounds: Vec<SoundOption>,
    timeline: Timeline,
    start_instant: Instant,

    connected: bool,
    oopz_status_text: String,
    hud_visible: bool,
    hud_active: bool,
    adjust_mode: bool,
    return_hwnd: HWND,

    // Active HUD state
    channel_menu_open: bool,
    hovered_channel_idx: Option<usize>,
    draft: String,
    ime_composing: bool,
    ime_composition: String,
    caret_visible: bool,
    scroll_offset_px: f32,
    copied_toast: Option<String>,
    notice_text: Option<String>,

    // HUD drag/resize state
    drag_kind: DragKind,
    drag_start_cursor: POINT,
    drag_orig_rect: RECT,

    // Settings UI state
    preview_night: bool,
    open_dropdown: OpenDropdown,
    dropdown_scroll: usize,
    hovered_dropdown_idx: Option<usize>,
    recording_hotkey: RecordingHotkey,
    active_slider: ActiveSlider,
    update_status: String,

    // On-demand timer flags (0% idle redraw)
    fade_timer_on: bool,
    caret_timer_on: bool,
    relaunch_exe: Option<PathBuf>,
}

thread_local! {
    static APP: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

fn with_app<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut AppState) -> R,
{
    APP.with(|cell| {
        if let Ok(mut guard) = cell.try_borrow_mut() {
            guard.as_mut().map(f)
        } else {
            None
        }
    })
}

pub fn run_app() -> Result<Option<PathBuf>, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let (mut settings, _migrated) = load_or_migrate_settings();
    settings.launch_on_startup = is_autostart_enabled();

    let gfx = GraphicsEngine::new()?;
    let (hx, hy, hw, hh) = compute_hud_rect(&settings);
    let hud = HudWindow::create(&gfx, Some(hud_wndproc), hx, hy, hw, hh)?;
    let settings_win = SettingsWindow::create(&gfx, Some(settings_wndproc))?;
    let tray = TrayIcon::new(hud.hwnd);
    let (gateway, gateway_rx) = GatewayHandle::new(hud.hwnd);
    let (update_tx, update_rx) = mpsc::channel();
    let sounds = enumerate_sounds();

    let mut state = AppState {
        gfx,
        hud,
        settings_win,
        tray,
        gateway,
        gateway_rx,
        update_rx,
        update_tx,
        settings,
        session: None,
        areas: Vec::new(),
        channels: Vec::new(),
        sounds,
        timeline: Timeline::default(),
        start_instant: Instant::now(),
        connected: false,
        oopz_status_text: "正在检测本机 Oopz 登录态…".to_string(),
        hud_visible: true,
        hud_active: false,
        adjust_mode: false,
        return_hwnd: HWND(std::ptr::null_mut()),
        channel_menu_open: false,
        hovered_channel_idx: None,
        draft: String::new(),
        ime_composing: false,
        ime_composition: String::new(),
        caret_visible: true,
        scroll_offset_px: 0.0,
        copied_toast: None,
        notice_text: None,
        drag_kind: DragKind::None,
        drag_start_cursor: POINT::default(),
        drag_orig_rect: RECT::default(),
        preview_night: false,
        open_dropdown: OpenDropdown::None,
        dropdown_scroll: 0,
        hovered_dropdown_idx: None,
        recording_hotkey: RecordingHotkey::None,
        active_slider: ActiveSlider::None,
        update_status: String::new(),
        fade_timer_on: false,
        caret_timer_on: false,
        relaunch_exe: None,
    };

    state.register_hotkeys();
    state.gateway.check_session();
    unsafe {
        let _ = SetTimer(state.hud.hwnd, TIMER_SESSION_POLL, 5000, None);
    }
    if state.settings.area_id.is_empty() || state.settings.channel_id.is_empty() {
        // First run or no channel chosen yet -> open Settings window
        state.open_settings();
    } else {
        state.hud.set_visible(true);
        state.render_hud();
    }

    APP.with(|cell| {
        *cell.borrow_mut() = Some(state);
    });

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    let mut relaunch_exe = None;
    APP.with(|cell| {
        if let Some(s) = cell.borrow_mut().take() {
            relaunch_exe = s.relaunch_exe.clone();
            s.gateway.disconnect();
            s.unregister_hotkeys();
        }
    });

    unsafe {
        CoUninitialize();
    }
    Ok(relaunch_exe)
}

fn compute_hud_rect(s: &AppSettings) -> (i32, i32, i32, i32) {
    unsafe {
        let sw = GetSystemMetrics(SM_CXSCREEN).max(1280);
        let sh = GetSystemMetrics(SM_CYSCREEN).max(720);
        let w = if s.overlay_width > 0 {
            s.overlay_width.clamp(280, sw)
        } else {
            ((sw as f64) * 0.24).round() as i32
        }
        .clamp(340, 560);
        let h = if s.overlay_height > 0 {
            s.overlay_height.clamp(150, sh)
        } else {
            ((sh as f64) * 0.22).round() as i32
        }
        .clamp(190, 360);
        let cx = ((sw as f64) * s.position_x.clamp(0.0, 1.0)).round() as i32;
        let cy = ((sh as f64) * s.position_y.clamp(0.0, 1.0)).round() as i32;
        let x = (cx - w / 2).clamp(12, (sw - w - 12).max(12));
        let y = (cy - h / 2).clamp(12, (sh - h - 12).max(12));
        (x, y, w, h)
    }
}

impl AppState {
    fn now_secs(&self) -> f64 {
        self.start_instant.elapsed().as_secs_f64()
    }

    fn register_hotkeys(&self) {
        self.unregister_hotkeys();
        unsafe {
            if let Ok(spec) = HotkeySpec::parse(&self.settings.hotkey) {
                let _ = RegisterHotKey(
                    self.hud.hwnd,
                    HOTKEY_ACTIVATE_ID,
                    windows::Win32::UI::Input::KeyboardAndMouse::HOT_KEY_MODIFIERS(
                        spec.modifier_flags(),
                    ),
                    spec.vk,
                );
            }
            if let Ok(spec) = HotkeySpec::parse(&self.settings.visibility_hotkey) {
                let _ = RegisterHotKey(
                    self.hud.hwnd,
                    HOTKEY_VISIBILITY_ID,
                    windows::Win32::UI::Input::KeyboardAndMouse::HOT_KEY_MODIFIERS(
                        spec.modifier_flags(),
                    ),
                    spec.vk,
                );
            }
        }
    }

    fn unregister_hotkeys(&self) {
        unsafe {
            let _ = UnregisterHotKey(self.hud.hwnd, HOTKEY_ACTIVATE_ID);
            let _ = UnregisterHotKey(self.hud.hwnd, HOTKEY_VISIBILITY_ID);
        }
    }

    fn sync_timers(&mut self) {
        let now = self.now_secs();
        let need_fade = !self.hud_active
            && !self.adjust_mode
            && !self.settings.always_visible
            && self.timeline.has_active_fade(now);
        if need_fade != self.fade_timer_on {
            self.fade_timer_on = need_fade;
            unsafe {
                if need_fade {
                    let _ = SetTimer(self.hud.hwnd, TIMER_FADE, 33, None);
                } else {
                    let _ = KillTimer(self.hud.hwnd, TIMER_FADE);
                }
            }
        }

        let need_caret = self.hud_active;
        if need_caret != self.caret_timer_on {
            self.caret_timer_on = need_caret;
            unsafe {
                if need_caret {
                    let _ = SetTimer(self.hud.hwnd, TIMER_CARET, 530, None);
                } else {
                    let _ = KillTimer(self.hud.hwnd, TIMER_CARET);
                }
            }
        }
    }

    fn render_hud(&mut self) {
        let now = self.now_secs();
        let messages = if self.adjust_mode {
            sample_preview_messages()
        } else {
            self.timeline.items()
        };
        let opacities: Vec<f32> = messages
            .iter()
            .map(|m| {
                if self.adjust_mode {
                    1.0
                } else {
                    self.timeline.opacity(&m.message_id, now)
                }
            })
            .collect();
        let ch_label = self.settings.channel_name.clone();
        self.hud.render(
            &self.gfx,
            &HudRenderInput {
                bounds: Default::default(),
                settings: &self.settings,
                messages: &messages,
                opacities: &opacities,
                active: self.hud_active,
                adjust_mode: self.adjust_mode,
                connected: self.connected,
                channel_label: &ch_label,
                channels: &self.channels,
                channel_menu_open: self.channel_menu_open,
                hovered_channel_idx: self.hovered_channel_idx,
                draft: &self.draft,
                ime_composition: &self.ime_composition,
                caret_visible: self.caret_visible,
                scroll_offset_px: self.scroll_offset_px,
                unread_count: self.timeline.unread,
                copied_toast: self.copied_toast.as_deref(),
                notice_text: self.notice_text.as_deref(),
            },
        );
        self.sync_timers();
    }

    fn render_settings(&mut self) {
        if !self.settings_win.visible {
            return;
        }
        let view = SettingsViewState {
            settings: &self.settings,
            connected: self.connected,
            oopz_status_text: &self.oopz_status_text,
            areas: &self.areas,
            channels: &self.channels,
            sounds: &self.sounds,
            preview_night: self.preview_night,
            open_dropdown: self.open_dropdown,
            dropdown_scroll: self.dropdown_scroll,
            hovered_dropdown_idx: self.hovered_dropdown_idx,
            recording_hotkey: self.recording_hotkey,
            update_status: &self.update_status,
        };
        self.settings_win.render(&self.gfx, &view);
    }

    fn open_settings(&mut self) {
        if self.hud_active {
            self.exit_active_mode(false);
        }
        self.open_dropdown = OpenDropdown::None;
        self.recording_hotkey = RecordingHotkey::None;
        self.settings_win.show();
        self.hud.set_visible(false);
        self.render_settings();
    }

    fn close_settings(&mut self) {
        self.open_dropdown = OpenDropdown::None;
        self.recording_hotkey = RecordingHotkey::None;
        self.settings_win.hide();
        self.hud.set_visible(self.hud_visible);
        if self.hud_visible {
            self.render_hud();
        }
    }

    fn enter_active_mode(&mut self) {
        if self.settings_win.visible {
            self.settings_win.hide();
        }
        self.return_hwnd = capture_foreground_window();
        self.hud_visible = true;
        self.hud_active = true;
        self.adjust_mode = false;
        self.caret_visible = true;
        self.scroll_offset_px = 0.0;
        self.timeline.mark_read();
        self.hud.set_visible(true);
        self.hud.focus_for_input();
        self.render_hud();
        self.hud.update_ime_position();
    }

    fn exit_active_mode(&mut self, restore_focus: bool) {
        let was_adjust = self.adjust_mode;
        self.hud_active = false;
        self.adjust_mode = false;
        self.channel_menu_open = false;
        self.ime_composing = false;
        self.ime_composition.clear();
        self.scroll_offset_px = 0.0;
        self.drag_kind = DragKind::None;
        self.hud.set_click_through(true);
        self.render_hud();
        if restore_focus {
            restore_foreground_window(self.return_hwnd, self.hud.hwnd);
        }
        if was_adjust {
            self.open_settings();
        }
    }

    fn enter_adjust_mode(&mut self) {
        self.settings_win.hide();
        self.adjust_mode = true;
        self.hud_active = false;
        self.hud_visible = true;
        self.hud.set_click_through(false);
        self.hud.set_visible(true);
        self.hud.focus_for_input();
        self.render_hud();
    }

    fn toggle_hud_visibility(&mut self) {
        if self.hud_active {
            self.exit_active_mode(true);
        }
        self.hud_visible = !self.hud_visible;
        if !self.settings_win.visible {
            self.hud.set_visible(self.hud_visible);
            if self.hud_visible {
                self.render_hud();
            }
        }
    }

    fn show_toast(&mut self, text: &str) {
        self.copied_toast = Some(text.to_string());
        unsafe {
            let _ = SetTimer(self.hud.hwnd, TIMER_TOAST, 1600, None);
        }
        self.render_hud();
    }

    fn persist_hud_geometry(&mut self) {
        let r = self.hud.current_rect();
        unsafe {
            let sw = GetSystemMetrics(SM_CXSCREEN).max(1) as f64;
            let sh = GetSystemMetrics(SM_CYSCREEN).max(1) as f64;
            let w = (r.right - r.left).max(260);
            let h = (r.bottom - r.top).max(135);
            let cx = (r.left as f64 + (w as f64) * 0.5) / sw;
            let cy = (r.top as f64 + (h as f64) * 0.5) / sh;
            self.settings.position_x = cx.clamp(0.0, 1.0);
            self.settings.position_y = cy.clamp(0.0, 1.0);
            self.settings.overlay_width = w;
            self.settings.overlay_height = h;
            self.settings.dock_corner = Some(DockCorner::from_position(
                self.settings.position_x,
                self.settings.position_y,
            ));
        }
        let _ = save_settings(&self.settings);
    }

    fn select_area(&mut self, area_id: String, area_name: String) {
        if self.settings.area_id == area_id {
            return;
        }
        self.settings.area_id = area_id.clone();
        self.settings.area_name = area_name.clone();
        self.settings.channel_id.clear();
        self.settings.channel_name.clear();
        self.channels.clear();
        self.timeline.clear();
        self.connected = false;
        self.oopz_status_text = format!("已选择服务器 · {area_name}");
        let _ = save_settings(&self.settings);
        if let Some(sess) = self.session.clone() {
            self.gateway.fetch_channels(sess, area_id, area_name);
        }
        self.render_settings();
        self.render_hud();
    }

    fn select_channel(&mut self, channel_id: String, channel_name: String) {
        self.settings.channel_id = channel_id.clone();
        self.settings.channel_name = channel_name.clone();
        self.timeline.clear();
        let _ = save_settings(&self.settings);
        if let Some(sess) = self.session.clone() {
            if !self.settings.area_id.is_empty() {
                self.gateway.connect_channel(
                    sess,
                    self.settings.area_id.clone(),
                    channel_id,
                    channel_name,
                );
            }
        }
        self.render_settings();
        self.render_hud();
    }

    fn drain_gateway_events(&mut self) {
        let mut hud_dirty = false;
        let mut settings_dirty = false;
        while let Ok(ev) = self.gateway_rx.try_recv() {
            match ev {
                GatewayEvent::SessionLoaded(Ok(sess)) => {
                    let changed = self.session.as_ref() != Some(&sess);
                    self.settings.device_id = sess.device_id.clone();
                    self.settings.person_uid = sess.person_uid.clone();
                    self.settings.jwt_token = sess.jwt_token.clone();
                    self.settings.app_version = sess.app_version.clone();
                    self.session = Some(sess.clone());
                    if changed {
                        self.oopz_status_text = if self.settings.area_name.is_empty() {
                            "已连接 Oopz · 正在拉取服务器…".to_string()
                        } else {
                            format!("已连接 Oopz · {}", self.settings.area_name)
                        };
                        self.gateway.fetch_areas(sess.clone());
                        if !self.settings.area_id.is_empty() && !self.settings.channel_id.is_empty() {
                            self.gateway.connect_channel(
                                sess,
                                self.settings.area_id.clone(),
                                self.settings.channel_id.clone(),
                                self.settings.channel_name.clone(),
                            );
                        }
                        settings_dirty = true;
                    }
                }
                GatewayEvent::SessionLoaded(Err(err)) => {
                    if self.session.is_none() {
                        self.oopz_status_text = err;
                        self.connected = false;
                        settings_dirty = true;
                    }
                }
                GatewayEvent::AreasLoaded { areas } => {
                    self.areas = areas;
                    if let Some(sess) = self.session.clone() {
                        if self.settings.area_id.is_empty() {
                            if let Some(first) = self.areas.first().cloned() {
                                self.settings.area_id = first.area_id.clone();
                                self.settings.area_name = first.name.clone();
                                self.oopz_status_text = format!("已连接 Oopz · {}", first.name);
                                self.gateway.fetch_channels(sess, first.area_id, first.name);
                            }
                        } else {
                            if let Some(found) =
                                self.areas.iter().find(|a| a.area_id == self.settings.area_id)
                            {
                                self.settings.area_name = found.name.clone();
                                self.oopz_status_text = format!("已连接 Oopz · {}", found.name);
                            }
                            self.gateway.fetch_channels(
                                sess,
                                self.settings.area_id.clone(),
                                self.settings.area_name.clone(),
                            );
                        }
                    }
                    settings_dirty = true;
                }
                GatewayEvent::ChannelsLoaded { area_id, channels } => {
                    if area_id == self.settings.area_id {
                        self.channels = channels;
                        if self.settings.channel_id.is_empty() {
                            if let Some(first) = self.channels.first().cloned() {
                                self.select_channel(first.channel_id, first.channel_name);
                            }
                        } else if let Some(found) = self
                            .channels
                            .iter()
                            .find(|c| c.channel_id == self.settings.channel_id)
                        {
                            self.settings.channel_name = found.channel_name.clone();
                        }
                        settings_dirty = true;
                        hud_dirty = true;
                    }
                }
                GatewayEvent::StatusChanged { connected, text } => {
                    self.connected = connected;
                    if connected && !self.settings.area_name.is_empty() {
                        self.oopz_status_text =
                            format!("已连接 Oopz · {}", self.settings.area_name);
                    } else if !connected {
                        self.notice_text = Some(text);
                    }
                    settings_dirty = true;
                    hud_dirty = true;
                }
                GatewayEvent::HistoryLoaded {
                    generation,
                    messages,
                } => {
                    if generation == self.gateway.current_generation() {
                        let now = self.now_secs();
                        self.timeline.merge(messages, now, false, true);
                        hud_dirty = true;
                    }
                }
                GatewayEvent::LiveMessage {
                    generation,
                    message,
                } => {
                    if generation == self.gateway.current_generation() {
                        let now = self.now_secs();
                        let reading_bottom = self.hud_active && self.scroll_offset_px <= 4.0;
                        let notify = self.timeline.merge(
                            std::iter::once(message),
                            now,
                            true,
                            reading_bottom,
                        );
                        if !notify.is_empty() && self.settings.notification_enabled {
                            play_sound(&self.settings.notification_sound, false);
                        }
                        hud_dirty = true;
                    }
                }
                GatewayEvent::MessageDeleted {
                    generation,
                    message_id,
                } => {
                    if generation == self.gateway.current_generation()
                        && self.timeline.remove_message(&message_id)
                    {
                        hud_dirty = true;
                    }
                }
                GatewayEvent::SendResult { ok, error } => {
                    if !ok {
                        if let Some(e) = error {
                            self.show_toast(&e);
                        }
                    }
                }
                GatewayEvent::Error(e) => {
                    self.notice_text = Some(e);
                    hud_dirty = true;
                    settings_dirty = true;
                }
            }
        }
        if hud_dirty && self.hud_visible && !self.settings_win.visible {
            self.render_hud();
        }
        if settings_dirty && self.settings_win.visible {
            self.render_settings();
        }
    }

    fn trigger_update_check(&mut self) {
        self.update_status = "正在检查更新…".to_string();
        self.render_settings();
        let tx = self.update_tx.clone();
        let hwnd = self.hud.hwnd.0 as usize;
        thread::spawn(move || {
            let res = check_and_apply_update();
            let _ = tx.send(res);
            unsafe {
                let _ = PostMessageW(
                    HWND(hwnd as *mut _),
                    WM_APP_UPDATE_RESULT,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        });
    }

    fn handle_update_results(&mut self) {
        while let Ok(res) = self.update_rx.try_recv() {
            match res {
                UpdateCheckResult::UpToDate(v) => {
                    self.update_status = format!("已是最新版本 v{v}");
                    self.render_settings();
                }
                UpdateCheckResult::UpdatedAndRestarting(v, exe_path) => {
                    self.update_status = format!("已更新至 v{v}，正在重启…");
                    self.relaunch_exe = Some(exe_path);
                    self.render_settings();
                    unsafe {
                        PostQuitMessage(0);
                    }
                }
                UpdateCheckResult::Error(e) => {
                    self.update_status = e;
                    self.render_settings();
                }
            }
        }
    }

    fn handle_hud_mouse_down(&mut self, x_dip: f32, y_dip: f32) {
        if !self.hud_active && !self.adjust_mode {
            return;
        }
        // 1. Channel dropdown menu item click
        if self.channel_menu_open {
            for item in self.hud.hits.channel_menu_items.clone() {
                if point_in_rect(x_dip, y_dip, item.rect) {
                    self.channel_menu_open = false;
                    self.select_channel(item.channel_id, item.channel_name);
                    return;
                }
            }
        }
        // 2. Top-bar channel selector pill click
        if self.hud_active && point_in_rect(x_dip, y_dip, self.hud.hits.channel_pill_rect) {
            self.channel_menu_open = !self.channel_menu_open;
            self.render_hud();
            return;
        }
        self.channel_menu_open = false;

        // 3. Unread jump-to-bottom pill
        if let Some(pill) = self.hud.hits.unread_pill_rect {
            if point_in_rect(x_dip, y_dip, pill) {
                self.scroll_offset_px = 0.0;
                self.timeline.mark_read();
                self.render_hud();
                return;
            }
        }

        // 4. Micro-capsule click to copy
        if self.hud_active {
            for cap in self.hud.hits.capsules.clone() {
                if point_in_rect(x_dip, y_dip, cap.rect) {
                    if copy_text_to_clipboard(self.hud.hwnd, &cap.copy_value) {
                        self.show_toast("已复制");
                    }
                    return;
                }
            }
        }

        // 5. Resize grip
        if point_in_rect(x_dip, y_dip, self.hud.hits.resize_grip_rect) {
            self.drag_kind = DragKind::Resize;
            unsafe {
                let _ = GetCursorPos(&mut self.drag_start_cursor);
                self.drag_orig_rect = self.hud.current_rect();
                SetCapture(self.hud.hwnd);
            }
            return;
        }

        // 6. Drag move via top bar or adjust mode body
        if self.adjust_mode || point_in_rect(x_dip, y_dip, self.hud.hits.top_bar_rect) {
            self.drag_kind = DragKind::Move;
            unsafe {
                let _ = GetCursorPos(&mut self.drag_start_cursor);
                self.drag_orig_rect = self.hud.current_rect();
                SetCapture(self.hud.hwnd);
            }
        }
    }

    fn handle_hud_mouse_move(&mut self, x_dip: f32, y_dip: f32) {
        if self.drag_kind != DragKind::None {
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
            }
            let dx = pt.x - self.drag_start_cursor.x;
            let dy = pt.y - self.drag_start_cursor.y;
            let orig = self.drag_orig_rect;
            match self.drag_kind {
                DragKind::Move => {
                    let w = orig.right - orig.left;
                    let h = orig.bottom - orig.top;
                    self.hud.set_bounds(orig.left + dx, orig.top + dy, w, h);
                }
                DragKind::Resize => {
                    let w = (orig.right - orig.left + dx).clamp(280, 900);
                    let h = (orig.bottom - orig.top + dy).clamp(150, 700);
                    self.hud.set_bounds(orig.left, orig.top, w, h);
                    self.render_hud();
                }
                DragKind::None => {}
            }
            return;
        }

        if self.channel_menu_open {
            let hovered = self
                .hud
                .hits
                .channel_menu_items
                .iter()
                .position(|it| point_in_rect(x_dip, y_dip, it.rect));
            if hovered != self.hovered_channel_idx {
                self.hovered_channel_idx = hovered;
                self.render_hud();
            }
        }
    }

    fn handle_hud_mouse_up(&mut self) {
        if self.drag_kind != DragKind::None {
            self.drag_kind = DragKind::None;
            unsafe {
                let _ = ReleaseCapture();
            }
            self.persist_hud_geometry();
            self.render_hud();
        }
    }

    fn handle_settings_mouse_down(&mut self, x: f32, y: f32) {
        let hits = self.settings_win.hits.clone();

        // 1. Open dropdown popup takes priority
        if self.open_dropdown != OpenDropdown::None {
            if let Some(pop) = hits.popup_rect {
                if point_in_rect(x, y, pop) {
                    for (row, idx) in hits.popup_items {
                        if point_in_rect(x, y, row) {
                            match self.open_dropdown {
                                OpenDropdown::Area => {
                                    if let Some(a) = self.areas.get(idx).cloned() {
                                        self.open_dropdown = OpenDropdown::None;
                                        self.select_area(a.area_id, a.name);
                                    }
                                }
                                OpenDropdown::Channel => {
                                    if let Some(c) = self.channels.get(idx).cloned() {
                                        self.open_dropdown = OpenDropdown::None;
                                        self.select_channel(c.channel_id, c.channel_name);
                                    }
                                }
                                OpenDropdown::Sound => {
                                    if let Some(s) = self.sounds.get(idx).cloned() {
                                        self.settings.notification_sound = s.key;
                                        let _ = save_settings(&self.settings);
                                        self.open_dropdown = OpenDropdown::None;
                                        self.render_settings();
                                    }
                                }
                                OpenDropdown::None => {}
                            }
                            return;
                        }
                    }
                    return;
                }
            }
            self.open_dropdown = OpenDropdown::None;
            self.render_settings();
            return;
        }

        // 2. Titlebar controls
        if point_in_rect(x, y, hits.btn_close) {
            self.close_settings();
            return;
        }
        if point_in_rect(x, y, hits.btn_minimize) {
            unsafe {
                let _ = ShowWindow(self.settings_win.hwnd, SW_MINIMIZE);
            }
            return;
        }
        if point_in_rect(x, y, hits.titlebar_rect) {
            unsafe {
                let _ = ReleaseCapture();
                let _ = SendMessageW(
                    self.settings_win.hwnd,
                    WM_NCLBUTTONDOWN,
                    WPARAM(HTCAPTION as usize),
                    LPARAM(0),
                );
            }
            return;
        }

        // 3. Preview buttons
        if point_in_rect(x, y, hits.btn_preview_sand) {
            self.preview_night = false;
            self.render_settings();
            return;
        }
        if point_in_rect(x, y, hits.btn_preview_night) {
            self.preview_night = true;
            self.render_settings();
            return;
        }
        if point_in_rect(x, y, hits.btn_drag_position) {
            self.enter_adjust_mode();
            return;
        }

        // 4. Dropdown triggers (Area / Channel / Sound)
        if point_in_rect(x, y, hits.dd_area) {
            self.open_dropdown = OpenDropdown::Area;
            self.dropdown_scroll = 0;
            self.hovered_dropdown_idx = None;
            self.render_settings();
            return;
        }
        if point_in_rect(x, y, hits.dd_channel) {
            self.open_dropdown = OpenDropdown::Channel;
            self.dropdown_scroll = 0;
            self.hovered_dropdown_idx = None;
            self.render_settings();
            return;
        }
        if point_in_rect(x, y, hits.dd_sound) {
            self.open_dropdown = OpenDropdown::Sound;
            self.dropdown_scroll = 0;
            self.hovered_dropdown_idx = None;
            self.render_settings();
            return;
        }
        if point_in_rect(x, y, hits.btn_preview_sound) {
            play_sound(&self.settings.notification_sound, true);
            return;
        }

        // 5. Hotkey recording pills
        if point_in_rect(x, y, hits.hk_activate) {
            self.recording_hotkey = RecordingHotkey::Activate;
            self.unregister_hotkeys();
            self.render_settings();
            return;
        }
        if point_in_rect(x, y, hits.hk_visibility) {
            self.recording_hotkey = RecordingHotkey::Visibility;
            self.unregister_hotkeys();
            self.render_settings();
            return;
        }

        // 6. 4-Quadrant dock corner selector
        for (corner, rect) in hits.dock_rects {
            if point_in_rect(x, y, rect) {
                self.settings.dock_corner = Some(corner);
                let (px, py) = corner.default_position();
                self.settings.position_x = px;
                self.settings.position_y = py;
                let (hx, hy, hw, hh) = compute_hud_rect(&self.settings);
                self.hud.set_bounds(hx, hy, hw, hh);
                let _ = save_settings(&self.settings);
                self.render_settings();
                return;
            }
        }

        // 7. Sliders
        if point_in_rect(x, y, hits.slider_font) {
            self.active_slider = ActiveSlider::FontSize;
            unsafe {
                SetCapture(self.settings_win.hwnd);
            }
            self.update_slider_from_x(x);
            return;
        }
        if point_in_rect(x, y, hits.slider_text_opacity) {
            self.active_slider = ActiveSlider::TextOpacity;
            unsafe {
                SetCapture(self.settings_win.hwnd);
            }
            self.update_slider_from_x(x);
            return;
        }
        if point_in_rect(x, y, hits.slider_backdrop) {
            self.active_slider = ActiveSlider::BackdropOpacity;
            unsafe {
                SetCapture(self.settings_win.hwnd);
            }
            self.update_slider_from_x(x);
            return;
        }

        // 8. Toggles
        if point_in_rect(x, y, hits.toggle_always_visible) {
            self.settings.always_visible = !self.settings.always_visible;
            let _ = save_settings(&self.settings);
            self.render_settings();
            return;
        }
        if point_in_rect(x, y, hits.toggle_notify_sound) {
            self.settings.notification_enabled = !self.settings.notification_enabled;
            let _ = save_settings(&self.settings);
            self.render_settings();
            return;
        }
        if point_in_rect(x, y, hits.toggle_autostart) {
            let next = !self.settings.launch_on_startup;
            if set_autostart_enabled(next).is_ok() {
                self.settings.launch_on_startup = next;
                let _ = save_settings(&self.settings);
            }
            self.render_settings();
            return;
        }

        // 9. Check Update button
        if point_in_rect(x, y, hits.btn_check_update) {
            self.trigger_update_check();
        }
    }

    fn update_slider_from_x(&mut self, x: f32) {
        let rect = match self.active_slider {
            ActiveSlider::FontSize => self.settings_win.hits.slider_font,
            ActiveSlider::TextOpacity => self.settings_win.hits.slider_text_opacity,
            ActiveSlider::BackdropOpacity => self.settings_win.hits.slider_backdrop,
            ActiveSlider::None => return,
        };
        let track_l = rect.left + 6.0;
        let track_r = rect.right - 6.0;
        let ratio = ((x - track_l) / (track_r - track_l).max(1.0)).clamp(0.0, 1.0);
        match self.active_slider {
            ActiveSlider::FontSize => {
                self.settings.font_size = (10.0 + ratio * 14.0).round() as i32;
            }
            ActiveSlider::TextOpacity => {
                self.settings.text_opacity = (20.0 + ratio * 80.0).round() as i32;
            }
            ActiveSlider::BackdropOpacity => {
                self.settings.backdrop_opacity = (ratio * 85.0).round() as i32;
            }
            ActiveSlider::None => {}
        }
        self.render_settings();
    }

    fn handle_settings_keydown(&mut self, vk: u32) {
        if self.recording_hotkey == RecordingHotkey::None {
            if vk == VK_ESCAPE.0 as u32 {
                if self.open_dropdown != OpenDropdown::None {
                    self.open_dropdown = OpenDropdown::None;
                    self.render_settings();
                } else {
                    self.close_settings();
                }
            }
            return;
        }

        if vk == VK_ESCAPE.0 as u32 {
            self.recording_hotkey = RecordingHotkey::None;
            self.register_hotkeys();
            self.render_settings();
            return;
        }
        // Ignore pure modifier presses
        if matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5) {
            return;
        }
        let ctrl = unsafe { (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
        let alt = unsafe { (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 };
        let shift = unsafe { (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
        let win = unsafe {
            (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0
                || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0
        };
        let spec = HotkeySpec {
            vk,
            ctrl,
            alt,
            shift,
            win,
        };
        let formatted = spec.format();
        match self.recording_hotkey {
            RecordingHotkey::Activate => self.settings.hotkey = formatted,
            RecordingHotkey::Visibility => self.settings.visibility_hotkey = formatted,
            RecordingHotkey::None => {}
        }
        self.recording_hotkey = RecordingHotkey::None;
        let _ = save_settings(&self.settings);
        self.register_hotkeys();
        self.render_settings();
    }
}

fn lparam_to_dip(lparam: LPARAM, dpi: f32) -> (f32, f32) {
    let x_px = (lparam.0 & 0xFFFF) as i16 as f32;
    let y_px = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    let scale = dpi / 96.0;
    (x_px / scale, y_px / scale)
}

unsafe extern "system" fn hud_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            let id = wparam.0 as i32;
            with_app(|app| {
                if id == HOTKEY_ACTIVATE_ID {
                    if app.hud_active {
                        app.exit_active_mode(true);
                    } else {
                        app.enter_active_mode();
                    }
                } else if id == HOTKEY_VISIBILITY_ID {
                    app.toggle_hud_visibility();
                }
            });
            LRESULT(0)
        }
        WM_APP_SHOW_SETTINGS => {
            with_app(|app| app.open_settings());
            LRESULT(0)
        }
        WM_APP_GATEWAY_EVENT => {
            with_app(|app| app.drain_gateway_events());
            LRESULT(0)
        }
        WM_APP_UPDATE_RESULT => {
            with_app(|app| app.handle_update_results());
            LRESULT(0)
        }
        WM_APP_TRAY => {
            let ev = (lparam.0 & 0xFFFF) as u32;
            if ev == WM_LBUTTONUP {
                with_app(|app| app.toggle_hud_visibility());
            } else if ev == WM_RBUTTONUP {
                with_app(|app| app.tray.show_context_menu(app.hud_visible));
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let cmd = wparam.0 & 0xFFFF;
            match cmd {
                CMD_TRAY_SETTINGS => {
                    with_app(|app| app.open_settings());
                }
                CMD_TRAY_TOGGLE_HUD => {
                    with_app(|app| app.toggle_hud_visibility());
                }
                CMD_TRAY_CHECK_UPDATE => {
                    with_app(|app| {
                        app.open_settings();
                        app.trigger_update_check();
                    });
                }
                CMD_TRAY_QUIT => {
                    PostQuitMessage(0);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let tid = wparam.0;
            with_app(|app| match tid {
                TIMER_FADE => app.render_hud(),
                TIMER_CARET => {
                    app.caret_visible = !app.caret_visible;
                    app.render_hud();
                }
                TIMER_TOAST => {
                    let _ = KillTimer(app.hud.hwnd, TIMER_TOAST);
                    app.copied_toast = None;
                    app.notice_text = None;
                    app.render_hud();
                }
                TIMER_SESSION_POLL => {
                    if app.session.is_none() || !app.connected {
                        app.gateway.check_session();
                    }
                }
                _ => {}
            });
            LRESULT(0)
        }
        WM_IME_STARTCOMPOSITION => {
            with_app(|app| {
                app.ime_composing = true;
                app.hud.update_ime_position();
            });
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_IME_COMPOSITION => {
            let handled = with_app(|app| {
                if let Some(result_str) = app.hud.read_ime_string(lparam, true) {
                    if !result_str.is_empty() {
                        app.draft.push_str(&result_str);
                        app.ime_composition.clear();
                        app.render_hud();
                        app.hud.update_ime_position();
                        return true;
                    }
                }
                if let Some(comp_str) = app.hud.read_ime_string(lparam, false) {
                    app.ime_composition = comp_str;
                    app.render_hud();
                    app.hud.update_ime_position();
                    return true;
                }
                false
            })
            .unwrap_or(false);
            if handled {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_IME_ENDCOMPOSITION => {
            with_app(|app| {
                app.ime_composing = false;
                app.ime_composition.clear();
                app.render_hud();
            });
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CHAR => {
            let ch_u32 = wparam.0 as u32;
            if ch_u32 >= 0x20 {
                if let Some(ch) = char::from_u32(ch_u32) {
                    with_app(|app| {
                        if app.hud_active && !app.ime_composing {
                            app.draft.push(ch);
                            app.caret_visible = true;
                            app.render_hud();
                            app.hud.update_ime_position();
                        }
                    });
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u32;
            with_app(|app| {
                if app.ime_composing {
                    return;
                }
                if vk == VK_ESCAPE.0 as u32 {
                    if app.drag_kind != DragKind::None {
                        let r = app.drag_orig_rect;
                        app.drag_kind = DragKind::None;
                        let _ = ReleaseCapture();
                        app.hud
                            .set_bounds(r.left, r.top, r.right - r.left, r.bottom - r.top);
                        app.render_hud();
                    } else if app.channel_menu_open {
                        app.channel_menu_open = false;
                        app.render_hud();
                    } else {
                        app.exit_active_mode(true);
                    }
                } else if vk == VK_TAB.0 as u32 && app.hud_active {
                    app.channel_menu_open = !app.channel_menu_open;
                    app.render_hud();
                } else if vk == VK_BACK.0 as u32 && app.hud_active {
                    app.draft.pop();
                    app.caret_visible = true;
                    app.render_hud();
                    app.hud.update_ime_position();
                } else if vk == VK_RETURN.0 as u32 && app.hud_active {
                    let text = app.draft.trim().to_string();
                    app.draft.clear();
                    if !text.is_empty() {
                        if let Some(sess) = app.session.clone() {
                            if !app.settings.area_id.is_empty()
                                && !app.settings.channel_id.is_empty()
                            {
                                app.gateway.send_message(
                                    sess,
                                    app.settings.area_id.clone(),
                                    app.settings.channel_id.clone(),
                                    text,
                                );
                            }
                        }
                    }
                    app.exit_active_mode(true);
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            with_app(|app| {
                let (x, y) = lparam_to_dip(lparam, app.hud.dpi());
                app.handle_hud_mouse_down(x, y);
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            with_app(|app| {
                let (x, y) = lparam_to_dip(lparam, app.hud.dpi());
                app.handle_hud_mouse_move(x, y);
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            with_app(|app| app.handle_hud_mouse_up());
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) as i16) as f32;
            let ctrl = (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
            with_app(|app| {
                if !app.hud_active {
                    return;
                }
                if ctrl {
                    let step = if delta > 0.0 { 1 } else { -1 };
                    app.settings.font_size = (app.settings.font_size + step).clamp(10, 24);
                    let _ = save_settings(&app.settings);
                } else {
                    let max_s = app.hud.hits.max_scroll_px;
                    app.scroll_offset_px =
                        (app.scroll_offset_px + (delta / 120.0) * 36.0).clamp(0.0, max_s);
                    if app.scroll_offset_px <= 2.0 {
                        app.timeline.mark_read();
                    }
                }
                app.render_hud();
            });
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn settings_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CLOSE => {
            with_app(|app| app.close_settings());
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            with_app(|app| {
                let (x, y) = lparam_to_dip(lparam, app.settings_win.dpi());
                app.handle_settings_mouse_down(x, y);
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            with_app(|app| {
                let (x, y) = lparam_to_dip(lparam, app.settings_win.dpi());
                if app.active_slider != ActiveSlider::None {
                    app.update_slider_from_x(x);
                } else if app.open_dropdown != OpenDropdown::None {
                    let hovered = app
                        .settings_win
                        .hits
                        .popup_items
                        .iter()
                        .find(|(r, _)| point_in_rect(x, y, *r))
                        .map(|(_, idx)| *idx);
                    if hovered != app.hovered_dropdown_idx {
                        app.hovered_dropdown_idx = hovered;
                        app.render_settings();
                    }
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            with_app(|app| {
                if app.active_slider != ActiveSlider::None {
                    app.active_slider = ActiveSlider::None;
                    let _ = ReleaseCapture();
                    let _ = save_settings(&app.settings);
                }
            });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = (wparam.0 >> 16) as i16;
            with_app(|app| {
                if app.open_dropdown != OpenDropdown::None {
                    if delta < 0 {
                        app.dropdown_scroll = app.dropdown_scroll.saturating_add(1);
                    } else {
                        app.dropdown_scroll = app.dropdown_scroll.saturating_sub(1);
                    }
                    app.render_settings();
                }
            });
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u32;
            with_app(|app| app.handle_settings_keydown(vk));
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}


