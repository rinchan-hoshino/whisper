use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP, D2D_POINT_2F, D2D_RECT_F,
    D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1DeviceContext, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL, D2D1_EXTEND_MODE_CLAMP,
    D2D1_GAMMA_2_2, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_SWEEP_DIRECTION_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetSystemMetrics, LoadCursorW, LoadIconW, RegisterClassW, SendMessageW,
    SetForegroundWindow, ShowWindow, CS_HREDRAW, CS_VREDRAW, ICON_BIG, ICON_SMALL, IDC_ARROW,
    IDI_APPLICATION, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_SHOW, WM_SETICON, WNDCLASSW, WNDPROC,
    WS_EX_NOREDIRECTIONBITMAP, WS_POPUP,
};

use crate::core::protocol::{AreaInfo, TextChannel};
use crate::core::settings::{AppSettings, DockCorner, APP_VERSION};
use crate::platform::d2d::{
    brush, draw_text_simple, fill_round_rect, rgba, stroke_round_rect, DCompTarget, GraphicsEngine,
};
use crate::platform::hud_render::{render_hud, sample_preview_messages, HudRenderInput};
use crate::platform::single_instance::SETTINGS_CLASS_NAME;
use crate::platform::sound::SoundOption;
use crate::platform::to_wide_null;

pub const SETTINGS_DIP_W: f32 = 620.0;
pub const SETTINGS_DIP_H: f32 = 816.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenDropdown {
    None,
    Area,
    Channel,
    Sound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingHotkey {
    None,
    Activate,
    Visibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveSlider {
    None,
    FontSize,
    TextOpacity,
    BackdropOpacity,
}

#[derive(Debug, Clone, Default)]
pub struct SettingsHitMap {
    pub titlebar_rect: D2D_RECT_F,
    pub btn_minimize: D2D_RECT_F,
    pub btn_close: D2D_RECT_F,
    pub btn_preview_sand: D2D_RECT_F,
    pub btn_preview_night: D2D_RECT_F,
    pub btn_drag_position: D2D_RECT_F,
    pub dd_area: D2D_RECT_F,
    pub dd_channel: D2D_RECT_F,
    pub hk_activate: D2D_RECT_F,
    pub hk_visibility: D2D_RECT_F,
    pub dock_rects: [(DockCorner, D2D_RECT_F); 4],
    pub slider_font: D2D_RECT_F,
    pub slider_text_opacity: D2D_RECT_F,
    pub slider_backdrop: D2D_RECT_F,
    pub toggle_always_visible: D2D_RECT_F,
    pub toggle_notify_sound: D2D_RECT_F,
    pub dd_sound: D2D_RECT_F,
    pub btn_preview_sound: D2D_RECT_F,
    pub toggle_autostart: D2D_RECT_F,
    pub btn_check_update: D2D_RECT_F,
    pub popup_rect: Option<D2D_RECT_F>,
    pub popup_items: Vec<(D2D_RECT_F, usize)>,
}

pub struct SettingsViewState<'a> {
    pub settings: &'a AppSettings,
    pub connected: bool,
    pub oopz_status_text: &'a str,
    pub areas: &'a [AreaInfo],
    pub channels: &'a [TextChannel],
    pub sounds: &'a [SoundOption],
    pub preview_night: bool,
    pub open_dropdown: OpenDropdown,
    pub dropdown_scroll: usize,
    pub hovered_dropdown_idx: Option<usize>,
    pub recording_hotkey: RecordingHotkey,
    pub update_status: &'a str,
}

pub struct SettingsWindow {
    pub hwnd: HWND,
    pub target: DCompTarget,
    pub hits: SettingsHitMap,
    pub visible: bool,
}

impl SettingsWindow {
    pub fn create(gfx: &GraphicsEngine, wndproc: WNDPROC) -> Result<Self, String> {
        let class_w = to_wide_null(SETTINGS_CLASS_NAME);
        let title_w = to_wide_null("Whisper 设置");
        unsafe {
            let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                .map_err(|e| format!("GetModuleHandleW failed: {e}"))?;
            let hicon = LoadIconW(hinst, PCWSTR(1usize as *const u16))
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))
                .unwrap_or_default();
            let hcursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: wndproc,
                hInstance: hinst.into(),
                hIcon: hicon,
                hCursor: hcursor,
                lpszClassName: PCWSTR(class_w.as_ptr()),
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);

            let screen_w = GetSystemMetrics(SM_CXSCREEN).max(1280);
            let screen_h = GetSystemMetrics(SM_CYSCREEN).max(800);
            let w_px = SETTINGS_DIP_W as i32;
            let h_px = SETTINGS_DIP_H as i32;
            let x = (screen_w - w_px) / 2;
            let y = ((screen_h - h_px) / 2).max(24);

            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP,
                PCWSTR(class_w.as_ptr()),
                PCWSTR(title_w.as_ptr()),
                WS_POPUP,
                x,
                y,
                w_px,
                h_px,
                None,
                None,
                hinst,
                None,
            )
            .map_err(|e| format!("CreateWindowExW Settings failed: {e}"))?;

            if !hicon.0.is_null() {
                let _ = SendMessageW(
                    hwnd,
                    WM_SETICON,
                    WPARAM(ICON_BIG as usize),
                    LPARAM(hicon.0 as isize),
                );
                let _ = SendMessageW(
                    hwnd,
                    WM_SETICON,
                    WPARAM(ICON_SMALL as usize),
                    LPARAM(hicon.0 as isize),
                );
            }

            let dpi = GetDpiForWindow(hwnd).max(96) as f32;
            let scale = dpi / 96.0;
            let scaled_w = (SETTINGS_DIP_W * scale).round() as i32;
            let scaled_h = (SETTINGS_DIP_H * scale).round() as i32;
            let sx = (screen_w - scaled_w) / 2;
            let sy = ((screen_h - scaled_h) / 2).max(16);
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                hwnd,
                None,
                sx,
                sy,
                scaled_w,
                scaled_h,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
            );

            let target = DCompTarget::new(gfx, hwnd, scaled_w as u32, scaled_h as u32, dpi)?;
            Ok(Self {
                hwnd,
                target,
                hits: SettingsHitMap::default(),
                visible: false,
            })
        }
    }

    pub fn dpi(&self) -> f32 {
        unsafe { GetDpiForWindow(self.hwnd).max(96) as f32 }
    }

    pub fn show(&mut self) {
        self.visible = true;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
        }
    }

    pub fn hide(&mut self) {
        self.visible = false;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    pub fn render(&mut self, gfx: &GraphicsEngine, view: &SettingsViewState<'_>) {
        let (w, h) = self.target.dip_size();
        self.target.begin_frame();
        self.hits = render_settings_ui(&self.target.dc, gfx, w, h, view);
        self.target.end_frame(gfx);
    }
}

fn render_settings_ui(
    dc: &ID2D1DeviceContext,
    gfx: &GraphicsEngine,
    w: f32,
    h: f32,
    view: &SettingsViewState<'_>,
) -> SettingsHitMap {
    let mut hits = SettingsHitMap::default();
    let outer = D2D_RECT_F {
        left: 0.5,
        top: 0.5,
        right: w - 0.5,
        bottom: h - 0.5,
    };

    // Outer dark slate window frame
    fill_round_rect(dc, outer, 14.0, rgba(16, 20, 28, 0.985));
    stroke_round_rect(dc, outer, 14.0, rgba(255, 255, 255, 0.11), 1.0);

    let title_fmt = gfx
        .create_text_format(
            13.5,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            false,
        )
        .unwrap();
    let label_fmt = gfx
        .create_text_format(
            13.0,
            DWRITE_FONT_WEIGHT_MEDIUM,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            false,
        )
        .unwrap();
    let muted_fmt = gfx
        .create_text_format(
            11.5,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            false,
        )
        .unwrap();
    let right_fmt = gfx
        .create_text_format(
            12.5,
            DWRITE_FONT_WEIGHT_MEDIUM,
            DWRITE_TEXT_ALIGNMENT_TRAILING,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            false,
        )
        .unwrap();
    let center_fmt = gfx
        .create_text_format(
            12.0,
            DWRITE_FONT_WEIGHT_MEDIUM,
            DWRITE_TEXT_ALIGNMENT_CENTER,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            false,
        )
        .unwrap();
    let bold_sm_fmt = gfx
        .create_text_format(
            11.0,
            DWRITE_FONT_WEIGHT_BOLD,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            false,
        )
        .unwrap();

    // --- 1. Titlebar (42px) ---
    hits.titlebar_rect = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: w - 76.0,
        bottom: 42.0,
    };
    // Whisper soundwave emblem badge (matches assets/whisper.ico)
    let badge = D2D_RECT_F {
        left: 14.0,
        top: 10.0,
        right: 36.0,
        bottom: 32.0,
    };
    draw_whisper_badge(dc, gfx, badge);
    draw_text_simple(
        dc,
        gfx,
        "Whisper 设置",
        &title_fmt,
        D2D_RECT_F {
            left: 44.0,
            top: 0.0,
            right: 240.0,
            bottom: 42.0,
        },
        rgba(241, 245, 249, 0.96),
    );

    // Minimize & Close buttons
    hits.btn_minimize = D2D_RECT_F {
        left: w - 72.0,
        top: 8.0,
        right: w - 42.0,
        bottom: 34.0,
    };
    hits.btn_close = D2D_RECT_F {
        left: w - 40.0,
        top: 8.0,
        right: w - 10.0,
        bottom: 34.0,
    };
    draw_text_simple(dc, gfx, "—", &center_fmt, hits.btn_minimize, rgba(148, 163, 184, 0.85));
    draw_text_simple(dc, gfx, "✕", &center_fmt, hits.btn_close, rgba(148, 163, 184, 0.85));

    let pad = 16.0;
    let card_w = w - pad * 2.0;

    // --- 2. LIVE PREVIEW · 1:1 Stage ---
    let stage = D2D_RECT_F {
        left: pad,
        top: 46.0,
        right: pad + card_w,
        bottom: 224.0,
    };
    draw_preview_backdrop(dc, stage, view.preview_night);
    stroke_round_rect(dc, stage, 10.0, rgba(255, 255, 255, 0.12), 1.0);

    draw_text_simple(
        dc,
        gfx,
        "LIVE PREVIEW · 1:1",
        &bold_sm_fmt,
        D2D_RECT_F {
            left: stage.left + 12.0,
            top: stage.top + 8.0,
            right: stage.left + 180.0,
            bottom: stage.top + 28.0,
        },
        if view.preview_night {
            rgba(203, 213, 225, 0.78)
        } else {
            rgba(71, 56, 36, 0.78)
        },
    );

    // Top-right preview mode controls
    let pill_drag = D2D_RECT_F {
        left: stage.right - 134.0,
        top: stage.top + 9.0,
        right: stage.right - 10.0,
        bottom: stage.top + 33.0,
    };
    hits.btn_drag_position = pill_drag;
    fill_round_rect(dc, pill_drag, 7.0, rgba(15, 20, 29, 0.78));
    stroke_round_rect(dc, pill_drag, 7.0, rgba(255, 255, 255, 0.16), 1.0);
    draw_text_simple(dc, gfx, "✥ 屏幕实景拖拽定位", &center_fmt, pill_drag, rgba(241, 245, 249, 0.94));

    let seg_box = D2D_RECT_F {
        left: pill_drag.left - 184.0,
        top: stage.top + 9.0,
        right: pill_drag.left - 8.0,
        bottom: stage.top + 33.0,
    };
    fill_round_rect(dc, seg_box, 7.0, rgba(15, 20, 29, 0.74));
    stroke_round_rect(dc, seg_box, 7.0, rgba(255, 255, 255, 0.14), 1.0);
    let mid_x = (seg_box.left + seg_box.right) * 0.5;
    hits.btn_preview_sand = D2D_RECT_F {
        left: seg_box.left + 2.0,
        top: seg_box.top + 2.0,
        right: mid_x,
        bottom: seg_box.bottom - 2.0,
    };
    hits.btn_preview_night = D2D_RECT_F {
        left: mid_x,
        top: seg_box.top + 2.0,
        right: seg_box.right - 2.0,
        bottom: seg_box.bottom - 2.0,
    };
    if !view.preview_night {
        fill_round_rect(dc, hits.btn_preview_sand, 5.0, rgba(255, 255, 255, 0.18));
    } else {
        fill_round_rect(dc, hits.btn_preview_night, 5.0, rgba(59, 130, 246, 0.35));
    }
    draw_text_simple(
        dc,
        gfx,
        "☀ 亮色沙地",
        &center_fmt,
        hits.btn_preview_sand,
        rgba(248, 250, 252, if !view.preview_night { 1.0 } else { 0.65 }),
    );
    draw_text_simple(
        dc,
        gfx,
        "🌙 暗夜室内",
        &center_fmt,
        hits.btn_preview_night,
        rgba(248, 250, 252, if view.preview_night { 1.0 } else { 0.65 }),
    );

    // Render 1:1 HUD inside preview stage using the exact same `render_hud` function!
    let preview_msgs = sample_preview_messages();
    let preview_ops = vec![1.0f32; preview_msgs.len()];
    let hud_box = match view.settings.effective_dock_corner() {
        DockCorner::TopLeft => D2D_RECT_F {
            left: stage.left + 8.0,
            top: stage.top + 36.0,
            right: stage.left + 390.0,
            bottom: stage.top + 148.0,
        },
        DockCorner::TopRight => D2D_RECT_F {
            left: stage.right - 390.0,
            top: stage.top + 36.0,
            right: stage.right - 8.0,
            bottom: stage.top + 148.0,
        },
        DockCorner::BottomLeft => D2D_RECT_F {
            left: stage.left + 8.0,
            top: stage.bottom - 116.0,
            right: stage.left + 390.0,
            bottom: stage.bottom - 6.0,
        },
        DockCorner::BottomRight => D2D_RECT_F {
            left: stage.right - 390.0,
            top: stage.bottom - 116.0,
            right: stage.right - 8.0,
            bottom: stage.bottom - 6.0,
        },
    };
    let mut preview_settings = view.settings.clone();
    preview_settings.always_visible = true;
    let _ = render_hud(
        dc,
        gfx,
        &HudRenderInput {
            bounds: hud_box,
            settings: &preview_settings,
            messages: &preview_msgs,
            opacities: &preview_ops,
            active: false,
            adjust_mode: false,
            connected: view.connected,
            channel_label: &view.settings.channel_name,
            channels: &[],
            channel_menu_open: false,
            hovered_channel_idx: None,
            draft: "",
            ime_composition: "",
            caret_visible: false,
            scroll_offset_px: 0.0,
            unread_count: 0,
            copied_toast: None,
            notice_text: None,
        },
    );
    draw_corner_bracket(dc, stage, view.settings.effective_dock_corner());

    // --- 3. Card 1: 频道连接 (Correction 1: Area & Text Channel dropdowns, NO voice-follow!) ---
    let card1 = D2D_RECT_F {
        left: pad,
        top: 236.0,
        right: pad + card_w,
        bottom: 342.0,
    };
    fill_round_rect(dc, card1, 10.0, rgba(22, 27, 38, 0.95));
    stroke_round_rect(dc, card1, 10.0, rgba(255, 255, 255, 0.07), 1.0);

    draw_text_simple(
        dc,
        gfx,
        "频道连接",
        &muted_fmt,
        D2D_RECT_F {
            left: card1.left + 16.0,
            top: card1.top + 10.0,
            right: card1.left + 140.0,
            bottom: card1.top + 28.0,
        },
        rgba(148, 163, 184, 0.85),
    );

    // Status dot + `已连接 Oopz · <当前域名称>`
    let dot_col = if view.connected {
        rgba(34, 197, 94, 1.0)
    } else {
        rgba(250, 204, 21, 0.95)
    };
    if let Ok(db) = brush(dc, dot_col) {
        unsafe {
            dc.FillEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: D2D_POINT_2F {
                        x: card1.left + 21.0,
                        y: card1.top + 43.0,
                    },
                    radiusX: 3.5,
                    radiusY: 3.5,
                },
                &db,
            );
        }
    }
    draw_text_simple(
        dc,
        gfx,
        view.oopz_status_text,
        &label_fmt,
        D2D_RECT_F {
            left: card1.left + 32.0,
            top: card1.top + 31.0,
            right: card1.right - 16.0,
            bottom: card1.top + 55.0,
        },
        rgba(241, 245, 249, 0.95),
    );

    // Two native dropdown selectors: 服务器（域） ▾ and # 文字频道 ▾
    let dd_w = (card_w - 44.0) * 0.5;
    hits.dd_area = D2D_RECT_F {
        left: card1.left + 16.0,
        top: card1.top + 62.0,
        right: card1.left + 16.0 + dd_w,
        bottom: card1.top + 94.0,
    };
    hits.dd_channel = D2D_RECT_F {
        left: hits.dd_area.right + 12.0,
        top: card1.top + 62.0,
        right: card1.right - 16.0,
        bottom: card1.top + 94.0,
    };

    let area_label = if view.settings.area_name.is_empty() {
        "选择服务器（域）".to_string()
    } else {
        view.settings.area_name.clone()
    };
    let ch_label = if view.settings.channel_name.is_empty() {
        "# 选择文字频道".to_string()
    } else {
        format!("# {}", view.settings.channel_name)
    };
    draw_dropdown_box(
        dc,
        gfx,
        hits.dd_area,
        &area_label,
        &label_fmt,
        view.open_dropdown == OpenDropdown::Area,
    );
    draw_dropdown_box(
        dc,
        gfx,
        hits.dd_channel,
        &ch_label,
        &label_fmt,
        view.open_dropdown == OpenDropdown::Channel,
    );

    // --- 4. Card 2: 全局热键 ---
    let card2 = D2D_RECT_F {
        left: pad,
        top: 354.0,
        right: pad + card_w,
        bottom: 438.0,
    };
    fill_round_rect(dc, card2, 10.0, rgba(22, 27, 38, 0.95));
    stroke_round_rect(dc, card2, 10.0, rgba(255, 255, 255, 0.07), 1.0);

    draw_text_simple(
        dc,
        gfx,
        "全局热键",
        &muted_fmt,
        D2D_RECT_F {
            left: card2.left + 16.0,
            top: card2.top + 10.0,
            right: card2.left + 140.0,
            bottom: card2.top + 28.0,
        },
        rgba(148, 163, 184, 0.85),
    );

    let hk_box_w = (card_w - 44.0) * 0.5;
    let hk1_box = D2D_RECT_F {
        left: card2.left + 16.0,
        top: card2.top + 36.0,
        right: card2.left + 16.0 + hk_box_w,
        bottom: card2.top + 72.0,
    };
    let hk2_box = D2D_RECT_F {
        left: hk1_box.right + 12.0,
        top: card2.top + 36.0,
        right: card2.right - 16.0,
        bottom: card2.top + 72.0,
    };
    hits.hk_activate = hk1_box;
    hits.hk_visibility = hk2_box;

    draw_hotkey_slot(
        dc,
        gfx,
        hk1_box,
        "呼出输入",
        &view.settings.hotkey,
        view.recording_hotkey == RecordingHotkey::Activate,
        &label_fmt,
        &center_fmt,
    );
    draw_hotkey_slot(
        dc,
        gfx,
        hk2_box,
        "显隐浮层",
        &view.settings.visibility_hotkey,
        view.recording_hotkey == RecordingHotkey::Visibility,
        &label_fmt,
        &center_fmt,
    );

    // --- 5. Card 3: 外观与行为 ---
    let card3 = D2D_RECT_F {
        left: pad,
        top: 450.0,
        right: pad + card_w,
        bottom: 760.0,
    };
    fill_round_rect(dc, card3, 10.0, rgba(22, 27, 38, 0.95));
    stroke_round_rect(dc, card3, 10.0, rgba(255, 255, 255, 0.07), 1.0);

    draw_text_simple(
        dc,
        gfx,
        "外观与行为",
        &muted_fmt,
        D2D_RECT_F {
            left: card3.left + 16.0,
            top: card3.top + 10.0,
            right: card3.left + 140.0,
            bottom: card3.top + 28.0,
        },
        rgba(148, 163, 184, 0.85),
    );

    // Dock corner 4-quadrant selector
    let row_y = card3.top + 36.0;
    draw_text_simple(
        dc,
        gfx,
        "停靠角落",
        &label_fmt,
        D2D_RECT_F {
            left: card3.left + 16.0,
            top: row_y,
            right: card3.left + 150.0,
            bottom: row_y + 28.0,
        },
        rgba(226, 232, 240, 0.95),
    );
    let corners = [
        DockCorner::TopLeft,
        DockCorner::TopRight,
        DockCorner::BottomLeft,
        DockCorner::BottomRight,
    ];
    let q_w = 34.0;
    let q_h = 22.0;
    let q_gap = 8.0;
    let q_start_x = card3.right - 16.0 - (q_w * 4.0 + q_gap * 3.0);
    let active_corner = view.settings.effective_dock_corner();
    for (idx, corner) in corners.iter().enumerate() {
        let r = D2D_RECT_F {
            left: q_start_x + (idx as f32) * (q_w + q_gap),
            top: row_y + 3.0,
            right: q_start_x + (idx as f32) * (q_w + q_gap) + q_w,
            bottom: row_y + 3.0 + q_h,
        };
        hits.dock_rects[idx] = (*corner, r);
        let selected = *corner == active_corner;
        fill_round_rect(
            dc,
            r,
            5.0,
            if selected {
                rgba(59, 130, 246, 0.24)
            } else {
                rgba(15, 19, 27, 0.85)
            },
        );
        stroke_round_rect(
            dc,
            r,
            5.0,
            if selected {
                rgba(59, 130, 246, 0.92)
            } else {
                rgba(255, 255, 255, 0.14)
            },
            1.2,
        );
        // Little indicator bar inside corner icon
        let ind = match corner {
            DockCorner::TopLeft => D2D_RECT_F {
                left: r.left + 4.0,
                top: r.top + 4.0,
                right: r.left + 14.0,
                bottom: r.top + 8.0,
            },
            DockCorner::TopRight => D2D_RECT_F {
                left: r.right - 14.0,
                top: r.top + 4.0,
                right: r.right - 4.0,
                bottom: r.top + 8.0,
            },
            DockCorner::BottomLeft => D2D_RECT_F {
                left: r.left + 4.0,
                top: r.bottom - 8.0,
                right: r.left + 14.0,
                bottom: r.bottom - 4.0,
            },
            DockCorner::BottomRight => D2D_RECT_F {
                left: r.right - 14.0,
                top: r.bottom - 8.0,
                right: r.right - 4.0,
                bottom: r.bottom - 4.0,
            },
        };
        fill_round_rect(
            dc,
            ind,
            2.0,
            if selected {
                rgba(147, 197, 253, 1.0)
            } else {
                rgba(148, 163, 184, 0.55)
            },
        );
    }

    // Sliders
    let s_left = card3.left + 126.0;
    let s_right = card3.right - 76.0;
    hits.slider_font = draw_slider_row(
        dc,
        gfx,
        card3.left + 16.0,
        card3.right - 16.0,
        s_left,
        s_right,
        card3.top + 78.0,
        "字号",
        &format!("{} px", view.settings.font_size),
        ((view.settings.font_size - 10) as f32) / 14.0,
        &label_fmt,
        &right_fmt,
    );
    hits.slider_text_opacity = draw_slider_row(
        dc,
        gfx,
        card3.left + 16.0,
        card3.right - 16.0,
        s_left,
        s_right,
        card3.top + 112.0,
        "文字不透明度",
        &format!("{}%", view.settings.text_opacity),
        ((view.settings.text_opacity - 20) as f32) / 80.0,
        &label_fmt,
        &right_fmt,
    );
    hits.slider_backdrop = draw_slider_row(
        dc,
        gfx,
        card3.left + 16.0,
        card3.right - 16.0,
        s_left,
        s_right,
        card3.top + 146.0,
        "底衬暗晕",
        &format!("{}%", view.settings.backdrop_opacity),
        (view.settings.backdrop_opacity as f32) / 85.0,
        &label_fmt,
        &right_fmt,
    );

    // Toggles + Correction 2 Sound Selector Row
    hits.toggle_always_visible = draw_toggle_row(
        dc,
        gfx,
        card3.left + 16.0,
        card3.right - 16.0,
        card3.top + 186.0,
        "无输入时常驻显示消息",
        Some("(8秒后淡出)"),
        view.settings.always_visible,
        &label_fmt,
        &muted_fmt,
    );
    hits.toggle_notify_sound = draw_toggle_row(
        dc,
        gfx,
        card3.left + 16.0,
        card3.right - 16.0,
        card3.top + 218.0,
        "收到他人消息轻响提示",
        None,
        view.settings.notification_enabled,
        &label_fmt,
        &muted_fmt,
    );

    // Correction 2: Notification sound dropdown + [▶ 试听] button
    let sound_y = card3.top + 250.0;
    draw_text_simple(
        dc,
        gfx,
        "提示音效",
        &muted_fmt,
        D2D_RECT_F {
            left: card3.left + 28.0,
            top: sound_y,
            right: card3.left + 110.0,
            bottom: sound_y + 28.0,
        },
        rgba(148, 163, 184, 0.90),
    );
    hits.btn_preview_sound = D2D_RECT_F {
        left: card3.right - 88.0,
        top: sound_y,
        right: card3.right - 16.0,
        bottom: sound_y + 28.0,
    };
    hits.dd_sound = D2D_RECT_F {
        left: card3.left + 110.0,
        top: sound_y,
        right: hits.btn_preview_sound.left - 8.0,
        bottom: sound_y + 28.0,
    };
    let sound_label = view
        .sounds
        .iter()
        .find(|s| s.key.eq_ignore_ascii_case(&view.settings.notification_sound))
        .map(|s| s.label.clone())
        .unwrap_or_else(|| {
            if view.settings.notification_sound.is_empty() {
                "选择提示音效".to_string()
            } else {
                view.settings.notification_sound.clone()
            }
        });
    draw_dropdown_box(
        dc,
        gfx,
        hits.dd_sound,
        &sound_label,
        &muted_fmt,
        view.open_dropdown == OpenDropdown::Sound,
    );
    fill_round_rect(dc, hits.btn_preview_sound, 7.0, rgba(30, 41, 59, 0.95));
    stroke_round_rect(dc, hits.btn_preview_sound, 7.0, rgba(255, 255, 255, 0.14), 1.0);
    draw_text_simple(
        dc,
        gfx,
        "▶ 试听",
        &center_fmt,
        hits.btn_preview_sound,
        rgba(226, 232, 240, 0.95),
    );

    hits.toggle_autostart = draw_toggle_row(
        dc,
        gfx,
        card3.left + 16.0,
        card3.right - 16.0,
        card3.top + 284.0,
        "开机静默自启",
        None,
        view.settings.launch_on_startup,
        &label_fmt,
        &muted_fmt,
    );

    // --- 6. Footer ---
    let foot_y = 772.0;
    let footer_left = if view.update_status.is_empty() {
        format!("Whisper v{APP_VERSION}")
    } else {
        format!("Whisper v{APP_VERSION} · {}", view.update_status)
    };
    draw_text_simple(
        dc,
        gfx,
        &footer_left,
        &muted_fmt,
        D2D_RECT_F {
            left: pad + 4.0,
            top: foot_y,
            right: w - 120.0,
            bottom: foot_y + 28.0,
        },
        rgba(148, 163, 184, 0.78),
    );

    hits.btn_check_update = D2D_RECT_F {
        left: w - pad - 88.0,
        top: foot_y,
        right: w - pad,
        bottom: foot_y + 28.0,
    };
    fill_round_rect(dc, hits.btn_check_update, 7.0, rgba(26, 32, 44, 0.95));
    stroke_round_rect(dc, hits.btn_check_update, 7.0, rgba(255, 255, 255, 0.12), 1.0);
    draw_text_simple(
        dc,
        gfx,
        "检查更新",
        &center_fmt,
        hits.btn_check_update,
        rgba(226, 232, 240, 0.92),
    );

    // --- 7. Open Dropdown Popup Overlay ---
    match view.open_dropdown {
        OpenDropdown::None => {}
        OpenDropdown::Area => {
            let items: Vec<(String, bool)> = view
                .areas
                .iter()
                .map(|a| (a.name.clone(), a.area_id == view.settings.area_id))
                .collect();
            draw_dropdown_popup(
                dc,
                gfx,
                hits.dd_area,
                &items,
                view.dropdown_scroll,
                view.hovered_dropdown_idx,
                false,
                &label_fmt,
                &mut hits,
            );
        }
        OpenDropdown::Channel => {
            let items: Vec<(String, bool)> = view
                .channels
                .iter()
                .map(|c| {
                    (
                        format!("# {}", c.channel_name),
                        c.channel_id == view.settings.channel_id,
                    )
                })
                .collect();
            draw_dropdown_popup(
                dc,
                gfx,
                hits.dd_channel,
                &items,
                view.dropdown_scroll,
                view.hovered_dropdown_idx,
                false,
                &label_fmt,
                &mut hits,
            );
        }
        OpenDropdown::Sound => {
            let items: Vec<(String, bool)> = view
                .sounds
                .iter()
                .map(|s| {
                    (
                        s.label.clone(),
                        s.key.eq_ignore_ascii_case(&view.settings.notification_sound),
                    )
                })
                .collect();
            draw_dropdown_popup(
                dc,
                gfx,
                hits.dd_sound,
                &items,
                view.dropdown_scroll,
                view.hovered_dropdown_idx,
                true,
                &muted_fmt,
                &mut hits,
            );
        }
    }

    hits
}

fn draw_whisper_badge(dc: &ID2D1DeviceContext, gfx: &GraphicsEngine, rect: D2D_RECT_F) {
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;
    // Deep sea slate base (#131824 -> #1D2638) + subtle #38BDF8 1px inner border
    fill_round_rect(dc, rect, 5.5, rgba(24, 32, 48, 1.0));
    stroke_round_rect(
        dc,
        D2D_RECT_F {
            left: rect.left + 0.5,
            top: rect.top + 0.5,
            right: rect.right - 0.5,
            bottom: rect.bottom - 0.5,
        },
        5.0,
        rgba(56, 189, 248, 0.55),
        1.0,
    );

    // Left rounded bubble sound-core (#F8FAFC)
    let core_cx = rect.left + w * 0.36;
    let core_cy = rect.top + h * 0.51;
    if let Ok(core_b) = brush(dc, rgba(248, 250, 252, 0.98)) {
        unsafe {
            dc.FillEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: D2D_POINT_2F {
                        x: core_cx,
                        y: core_cy,
                    },
                    radiusX: w * 0.145,
                    radiusY: h * 0.145,
                },
                &core_b,
            );
        }
    }

    // Two right circular-arc sound waves (#38BDF8 and #60A5FA)
    let arc_cx = rect.left + w * 0.43;
    let arc_cy = rect.top + h * 0.50;
    draw_badge_arc(dc, gfx, arc_cx, arc_cy, w * 0.22, 42.0, rgba(56, 189, 248, 1.0), 1.65);
    draw_badge_arc(dc, gfx, arc_cx, arc_cy, w * 0.35, 48.0, rgba(96, 165, 250, 0.95), 1.65);
}

#[allow(clippy::too_many_arguments)]
fn draw_badge_arc(
    dc: &ID2D1DeviceContext,
    gfx: &GraphicsEngine,
    cx: f32,
    cy: f32,
    radius: f32,
    half_angle_deg: f32,
    color: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
    stroke_w: f32,
) {
    let rad = half_angle_deg.to_radians();
    let start = D2D_POINT_2F {
        x: cx + radius * rad.cos(),
        y: cy - radius * rad.sin(),
    };
    let end = D2D_POINT_2F {
        x: cx + radius * rad.cos(),
        y: cy + radius * rad.sin(),
    };
    unsafe {
        if let (Ok(geom), Ok(b)) = (gfx.d2d_factory.CreatePathGeometry(), brush(dc, color)) {
            if let Ok(sink) = geom.Open() {
                sink.BeginFigure(start, D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: end,
                    size: D2D_SIZE_F {
                        width: radius,
                        height: radius,
                    },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: D2D1_ARC_SIZE_SMALL,
                });
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                let _ = sink.Close();
                dc.DrawGeometry(&geom, &b, stroke_w, &gfx.round_stroke_style);
            }
        }
    }
}

fn draw_preview_backdrop(dc: &ID2D1DeviceContext, rect: D2D_RECT_F, night: bool) {
    let stops = if night {
        [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: rgba(24, 30, 44, 1.0),
            },
            D2D1_GRADIENT_STOP {
                position: 0.6,
                color: rgba(15, 20, 30, 1.0),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: rgba(10, 13, 20, 1.0),
            },
        ]
    } else {
        [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: rgba(241, 221, 179, 1.0),
            },
            D2D1_GRADIENT_STOP {
                position: 0.55,
                color: rgba(226, 195, 140, 1.0),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: rgba(196, 160, 108, 1.0),
            },
        ]
    };
    unsafe {
        let rt: &windows::Win32::Graphics::Direct2D::ID2D1RenderTarget = dc;
        if let Ok(sc) = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) {
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: D2D_POINT_2F {
                    x: rect.left,
                    y: rect.top,
                },
                endPoint: D2D_POINT_2F {
                    x: rect.right,
                    y: rect.bottom,
                },
            };
            if let Ok(lb) = rt.CreateLinearGradientBrush(&props, None, &sc) {
                let rr = windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: 10.0,
                    radiusY: 10.0,
                };
                dc.FillRoundedRectangle(&rr, &lb);
            }
        }
    }
}

fn draw_corner_bracket(dc: &ID2D1DeviceContext, stage: D2D_RECT_F, corner: DockCorner) {
    if let Ok(b) = brush(dc, rgba(59, 130, 246, 0.9)) {
        let m = 10.0;
        let len = 14.0;
        let (x, y, dx, dy) = match corner {
            DockCorner::TopLeft => (stage.left + m, stage.top + 36.0, len, len),
            DockCorner::TopRight => (stage.right - m, stage.top + 36.0, -len, len),
            DockCorner::BottomLeft => (stage.left + m, stage.bottom - m, len, -len),
            DockCorner::BottomRight => (stage.right - m, stage.bottom - m, -len, -len),
        };
        unsafe {
            dc.DrawLine(D2D_POINT_2F { x, y }, D2D_POINT_2F { x: x + dx, y }, &b, 2.0, None);
            dc.DrawLine(D2D_POINT_2F { x, y }, D2D_POINT_2F { x, y: y + dy }, &b, 2.0, None);
        }
    }
}

fn draw_dropdown_box(
    dc: &ID2D1DeviceContext,
    gfx: &GraphicsEngine,
    rect: D2D_RECT_F,
    text: &str,
    fmt: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    open: bool,
) {
    fill_round_rect(dc, rect, 7.0, rgba(14, 18, 26, 0.95));
    stroke_round_rect(
        dc,
        rect,
        7.0,
        if open {
            rgba(59, 130, 246, 0.9)
        } else {
            rgba(255, 255, 255, 0.12)
        },
        1.0,
    );
    draw_text_simple(
        dc,
        gfx,
        text,
        fmt,
        D2D_RECT_F {
            left: rect.left + 10.0,
            top: rect.top,
            right: rect.right - 24.0,
            bottom: rect.bottom,
        },
        rgba(241, 245, 249, 0.95),
    );
    draw_text_simple(
        dc,
        gfx,
        if open { "▴" } else { "▾" },
        fmt,
        D2D_RECT_F {
            left: rect.right - 22.0,
            top: rect.top,
            right: rect.right - 6.0,
            bottom: rect.bottom,
        },
        rgba(148, 163, 184, 0.85),
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_hotkey_slot(
    dc: &ID2D1DeviceContext,
    gfx: &GraphicsEngine,
    rect: D2D_RECT_F,
    label: &str,
    hotkey: &str,
    recording: bool,
    label_fmt: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    center_fmt: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
) {
    fill_round_rect(dc, rect, 8.0, rgba(14, 18, 26, 0.92));
    stroke_round_rect(
        dc,
        rect,
        8.0,
        if recording {
            rgba(59, 130, 246, 0.95)
        } else {
            rgba(255, 255, 255, 0.08)
        },
        1.0,
    );
    draw_text_simple(
        dc,
        gfx,
        label,
        label_fmt,
        D2D_RECT_F {
            left: rect.left + 12.0,
            top: rect.top,
            right: rect.left + 100.0,
            bottom: rect.bottom,
        },
        rgba(226, 232, 240, 0.95),
    );

    let pill_w = if recording { 96.0 } else { 58.0 };
    let pill = D2D_RECT_F {
        left: rect.right - pill_w - 8.0,
        top: rect.top + 6.0,
        right: rect.right - 8.0,
        bottom: rect.bottom - 6.0,
    };
    fill_round_rect(
        dc,
        pill,
        6.0,
        if recording {
            rgba(37, 99, 235, 0.25)
        } else {
            rgba(30, 41, 59, 0.92)
        },
    );
    stroke_round_rect(
        dc,
        pill,
        6.0,
        if recording {
            rgba(59, 130, 246, 0.95)
        } else {
            rgba(255, 255, 255, 0.14)
        },
        1.0,
    );
    draw_text_simple(
        dc,
        gfx,
        if recording { "按下快捷键…" } else { hotkey },
        center_fmt,
        pill,
        if recording {
            rgba(147, 197, 253, 1.0)
        } else {
            rgba(241, 245, 249, 0.95)
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_slider_row(
    dc: &ID2D1DeviceContext,
    gfx: &GraphicsEngine,
    left: f32,
    right: f32,
    track_l: f32,
    track_r: f32,
    y: f32,
    label: &str,
    value_text: &str,
    ratio: f32,
    label_fmt: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    right_fmt: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
) -> D2D_RECT_F {
    draw_text_simple(
        dc,
        gfx,
        label,
        label_fmt,
        D2D_RECT_F {
            left,
            top: y,
            right: track_l - 8.0,
            bottom: y + 24.0,
        },
        rgba(226, 232, 240, 0.95),
    );
    draw_text_simple(
        dc,
        gfx,
        value_text,
        right_fmt,
        D2D_RECT_F {
            left: track_r + 8.0,
            top: y,
            right,
            bottom: y + 24.0,
        },
        rgba(148, 163, 184, 0.92),
    );

    let cy = y + 12.0;
    let clamped = ratio.clamp(0.0, 1.0);
    let knob_x = track_l + (track_r - track_l) * clamped;

    fill_round_rect(
        dc,
        D2D_RECT_F {
            left: track_l,
            top: cy - 2.0,
            right: track_r,
            bottom: cy + 2.0,
        },
        2.0,
        rgba(51, 65, 85, 0.85),
    );
    fill_round_rect(
        dc,
        D2D_RECT_F {
            left: track_l,
            top: cy - 2.0,
            right: knob_x,
            bottom: cy + 2.0,
        },
        2.0,
        rgba(226, 232, 240, 0.92),
    );

    if let Ok(kb) = brush(dc, rgba(255, 255, 255, 1.0)) {
        unsafe {
            dc.FillEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: D2D_POINT_2F { x: knob_x, y: cy },
                    radiusX: 6.5,
                    radiusY: 6.5,
                },
                &kb,
            );
        }
    }

    D2D_RECT_F {
        left: track_l - 6.0,
        top: y,
        right: track_r + 6.0,
        bottom: y + 24.0,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_toggle_row(
    dc: &ID2D1DeviceContext,
    gfx: &GraphicsEngine,
    left: f32,
    right: f32,
    y: f32,
    label: &str,
    sub: Option<&str>,
    on: bool,
    label_fmt: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    muted_fmt: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
) -> D2D_RECT_F {
    let (lw, _) = gfx.measure_text(label, label_fmt, 260.0);
    draw_text_simple(
        dc,
        gfx,
        label,
        label_fmt,
        D2D_RECT_F {
            left,
            top: y,
            right: left + lw + 6.0,
            bottom: y + 24.0,
        },
        rgba(226, 232, 240, 0.95),
    );
    if let Some(sub_text) = sub {
        draw_text_simple(
            dc,
            gfx,
            sub_text,
            muted_fmt,
            D2D_RECT_F {
                left: left + lw + 10.0,
                top: y,
                right: right - 54.0,
                bottom: y + 24.0,
            },
            rgba(148, 163, 184, 0.72),
        );
    }

    let sw = D2D_RECT_F {
        left: right - 40.0,
        top: y + 2.0,
        right,
        bottom: y + 22.0,
    };
    fill_round_rect(
        dc,
        sw,
        10.0,
        if on {
            rgba(59, 130, 246, 0.96)
        } else {
            rgba(51, 65, 85, 0.85)
        },
    );
    let knob_x = if on { sw.right - 10.0 } else { sw.left + 10.0 };
    if let Ok(kb) = brush(dc, rgba(255, 255, 255, 1.0)) {
        unsafe {
            dc.FillEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: D2D_POINT_2F {
                        x: knob_x,
                        y: (sw.top + sw.bottom) * 0.5,
                    },
                    radiusX: 7.5,
                    radiusY: 7.5,
                },
                &kb,
            );
        }
    }
    sw
}

#[allow(clippy::too_many_arguments)]
fn draw_dropdown_popup(
    dc: &ID2D1DeviceContext,
    gfx: &GraphicsEngine,
    anchor: D2D_RECT_F,
    items: &[(String, bool)],
    scroll: usize,
    hovered_idx: Option<usize>,
    open_upward: bool,
    fmt: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    hits: &mut SettingsHitMap,
) {
    let max_vis = 6usize;
    let vis_count = items.len().min(max_vis).max(1);
    let row_h = 28.0;
    let popup_h = (vis_count as f32) * row_h + 8.0;
    let popup = if open_upward {
        D2D_RECT_F {
            left: anchor.left,
            top: (anchor.top - popup_h - 4.0).max(48.0),
            right: anchor.right,
            bottom: anchor.top - 4.0,
        }
    } else {
        D2D_RECT_F {
            left: anchor.left,
            top: anchor.bottom + 4.0,
            right: anchor.right,
            bottom: anchor.bottom + 4.0 + popup_h,
        }
    };
    fill_round_rect(dc, popup, 8.0, rgba(13, 17, 25, 0.99));
    stroke_round_rect(dc, popup, 8.0, rgba(59, 130, 246, 0.55), 1.0);
    hits.popup_rect = Some(popup);

    if items.is_empty() {
        draw_text_simple(
            dc,
            gfx,
            "暂无可选项",
            fmt,
            D2D_RECT_F {
                left: popup.left + 10.0,
                top: popup.top + 4.0,
                right: popup.right - 10.0,
                bottom: popup.bottom - 4.0,
            },
            rgba(148, 163, 184, 0.7),
        );
        return;
    }

    let max_scroll = items.len().saturating_sub(max_vis);
    let start = scroll.min(max_scroll);
    for (slot, (idx, (label, selected))) in items.iter().enumerate().skip(start).take(max_vis).enumerate() {
        let row = D2D_RECT_F {
            left: popup.left + 4.0,
            top: popup.top + 4.0 + (slot as f32) * row_h,
            right: popup.right - 4.0,
            bottom: popup.top + 4.0 + ((slot + 1) as f32) * row_h,
        };
        if *selected {
            fill_round_rect(dc, row, 5.0, rgba(59, 130, 246, 0.28));
        } else if hovered_idx == Some(idx) {
            fill_round_rect(dc, row, 5.0, rgba(255, 255, 255, 0.08));
        }
        draw_text_simple(
            dc,
            gfx,
            label,
            fmt,
            D2D_RECT_F {
                left: row.left + 8.0,
                top: row.top,
                right: row.right - 8.0,
                bottom: row.bottom,
            },
            if *selected {
                rgba(147, 197, 253, 1.0)
            } else {
                rgba(226, 232, 240, 0.94)
            },
        );
        hits.popup_items.push((row, idx));
    }
}

#[allow(dead_code)]
fn _keep_rect(_: RECT) {}
