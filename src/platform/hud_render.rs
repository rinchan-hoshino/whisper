use windows::Win32::Graphics::Direct2D::Common::{D2D1_GRADIENT_STOP, D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1DeviceContext, ID2D1RenderTarget, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2,
    D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
};

use crate::core::capsules::{parse_segments, TextSegment};
use crate::core::lifecycle::top_bar_status_label;
use crate::core::protocol::TextChannel;
use crate::core::settings::{AppSettings, DockCorner};
use crate::core::timeline::{ChatMessage, Timeline};
use crate::platform::d2d::{
    brush, draw_text_outlined, draw_text_simple, fill_round_rect, rgba, stroke_round_rect,
    GraphicsEngine,
};

#[derive(Debug, Clone)]
pub struct CapsuleHitTarget {
    pub rect: D2D_RECT_F,
    pub copy_value: String,
}

#[derive(Debug, Clone)]
pub struct ChannelMenuHitTarget {
    pub rect: D2D_RECT_F,
    pub channel_id: String,
    pub channel_name: String,
}

#[derive(Debug, Clone, Default)]
pub struct HudHitMap {
    pub top_bar_rect: D2D_RECT_F,
    pub channel_pill_rect: D2D_RECT_F,
    pub input_rect: D2D_RECT_F,
    pub unread_pill_rect: Option<D2D_RECT_F>,
    pub resize_grip_rect: D2D_RECT_F,
    pub capsules: Vec<CapsuleHitTarget>,
    pub channel_menu_items: Vec<ChannelMenuHitTarget>,
    pub caret_pos: (f32, f32),
    pub max_scroll_px: f32,
}

pub struct HudRenderInput<'a> {
    pub bounds: D2D_RECT_F,
    pub settings: &'a AppSettings,
    pub messages: &'a [ChatMessage],
    pub opacities: &'a [f32],
    pub active: bool,
    pub adjust_mode: bool,
    pub connected: bool,
    pub channel_label: &'a str,
    pub channels: &'a [TextChannel],
    pub channel_menu_open: bool,
    pub hovered_channel_idx: Option<usize>,
    pub draft: &'a str,
    pub ime_composition: &'a str,
    pub caret_visible: bool,
    pub scroll_offset_px: f32,
    pub unread_count: usize,
    pub copied_toast: Option<&'a str>,
    pub notice_text: Option<&'a str>,
}

/// Shared 1:1 Direct2D HUD renderer used by BOTH the live HUD window AND the Settings Live Preview stage!
pub fn render_hud(
    dc: &ID2D1DeviceContext,
    gfx: &GraphicsEngine,
    input: &HudRenderInput<'_>,
) -> HudHitMap {
    let mut hits = HudHitMap::default();
    let b = input.bounds;
    let w = (b.right - b.left).max(1.0);
    let _h = (b.bottom - b.top).max(1.0);

    unsafe {
        dc.PushAxisAlignedClip(&b, windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    }

    let font_size = input.settings.font_size.clamp(10, 24) as f32;
    let text_alpha_scale = (input.settings.text_opacity.clamp(20, 100) as f32) / 100.0;
    let backdrop_alpha = (input.settings.backdrop_opacity.clamp(0, 85) as f32) / 100.0;

    if input.active || input.adjust_mode {
        // Active F8 / Adjust mode: dark translucent rounded panel + 1px inner highlight
        fill_round_rect(dc, b, 12.0, rgba(16, 19, 25, 0.84));
        stroke_round_rect(
            dc,
            D2D_RECT_F {
                left: b.left + 0.5,
                top: b.top + 0.5,
                right: b.right - 0.5,
                bottom: b.bottom - 0.5,
            },
            12.0,
            if input.adjust_mode {
                rgba(59, 130, 246, 0.72)
            } else {
                rgba(255, 255, 255, 0.14)
            },
            1.0,
        );
    } else if backdrop_alpha > 0.005 && !input.messages.is_empty() {
        // Passive mode: zero box, smooth radial gradient dark halo anchored to dock corner
        draw_passive_radial_backdrop(dc, b, input.settings.effective_dock_corner(), backdrop_alpha);
    }

    let top_bar_h = if input.active || input.adjust_mode { 30.0 } else { 0.0 };
    let input_bar_h = if input.active { 38.0 } else { 0.0 };
    let pad_x = 12.0;

    // 1. Active Top Bar (Correction 1: NO voice-follow badge; clean green dot + `# channel ▾` + right hints)
    if input.active || input.adjust_mode {
        let bar = D2D_RECT_F {
            left: b.left,
            top: b.top,
            right: b.right,
            bottom: b.top + top_bar_h,
        };
        hits.top_bar_rect = bar;

        // Subtle divider line below top bar
        if let Ok(div_b) = brush(dc, rgba(255, 255, 255, 0.08)) {
            unsafe {
                dc.DrawLine(
                    D2D_POINT_2F {
                        x: b.left + 10.0,
                        y: bar.bottom,
                    },
                    D2D_POINT_2F {
                        x: b.right - 10.0,
                        y: bar.bottom,
                    },
                    &div_b,
                    1.0,
                    None,
                );
            }
        }

        // Status dot
        let dot_color = if input.connected {
            rgba(34, 197, 94, 1.0)
        } else {
            rgba(250, 204, 21, 0.95)
        };
        if let Ok(dot_b) = brush(dc, dot_color) {
            unsafe {
                dc.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: D2D_POINT_2F {
                            x: b.left + 16.0,
                            y: b.top + top_bar_h * 0.5,
                        },
                        radiusX: 3.2,
                        radiusY: 3.2,
                    },
                    &dot_b,
                );
            }
        }

        // Channel selector pill (`# 战术大厅 ▾`)
        let mut pill_right = b.left + 120.0;
        let ch_text = if input.channel_label.is_empty() {
            "# 未选择频道 ▾".to_string()
        } else {
            format!("# {} ▾", input.channel_label.trim_start_matches('#'))
        };
        if let Ok(fmt) = gfx.create_text_format(
            12.0,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            false,
        ) {
            let (tw, _) = gfx.measure_text(&ch_text, &fmt, 200.0);
            let pill = D2D_RECT_F {
                left: b.left + 25.0,
                top: b.top + 4.0,
                right: (b.left + 25.0 + tw + 14.0).min(b.right - 120.0),
                bottom: b.top + top_bar_h - 4.0,
            };
            pill_right = pill.right;
            hits.channel_pill_rect = pill;
            if input.channel_menu_open {
                fill_round_rect(dc, pill, 6.0, rgba(255, 255, 255, 0.12));
            }
            draw_text_simple(
                dc,
                gfx,
                &ch_text,
                &fmt,
                D2D_RECT_F {
                    left: pill.left + 6.0,
                    top: pill.top,
                    right: pill.right - 4.0,
                    bottom: pill.bottom,
                },
                rgba(241, 245, 249, 0.96),
            );
        }

        // Right side of top bar: transient copied toast > amber disconnect/waiting status > subtle hints
        if input.active && input.copied_toast.is_some() {
            if let Some(toast) = input.copied_toast {
                if let Ok(tfmt) = gfx.create_text_format(
                    10.5,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                    false,
                ) {
                    let max_w = (b.right - pill_right - 18.0).max(52.0);
                    let (tw, _) = gfx.measure_text(toast, &tfmt, max_w - 16.0);
                    let pw = (tw + 16.0).min(max_w);
                    let pill = D2D_RECT_F {
                        left: b.right - pw - 10.0,
                        top: b.top + 4.0,
                        right: b.right - 10.0,
                        bottom: b.top + top_bar_h - 4.0,
                    };
                    fill_round_rect(dc, pill, 10.0, rgba(16, 185, 129, 0.24));
                    stroke_round_rect(dc, pill, 10.0, rgba(52, 211, 153, 0.55), 1.0);
                    draw_text_simple(dc, gfx, toast, &tfmt, pill, rgba(167, 243, 208, 0.98));
                }
            }
        } else if let Some(status) = top_bar_status_label(input.connected, input.notice_text) {
            if let Ok(sfmt) = gfx.create_text_format(
                10.5,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                false,
            ) {
                draw_text_simple(
                    dc,
                    gfx,
                    &status,
                    &sfmt,
                    D2D_RECT_F {
                        left: (pill_right + 8.0).min(b.right - 72.0),
                        top: b.top,
                        right: b.right - 12.0,
                        bottom: b.top + top_bar_h,
                    },
                    rgba(251, 191, 36, 0.92),
                );
            }
        } else {
            let hint = if input.adjust_mode {
                "拖动定位 · 右下角缩放 · Esc 完成"
            } else {
                "Tab 切频 · 滚轮翻阅 · Esc 回游戏"
            };
            if let Ok(hfmt) = gfx.create_text_format(
                10.5,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                false,
            ) {
                draw_text_simple(
                    dc,
                    gfx,
                    hint,
                    &hfmt,
                    D2D_RECT_F {
                        left: (pill_right + 8.0).min(b.right - 120.0),
                        top: b.top,
                        right: b.right - 12.0,
                        bottom: b.top + top_bar_h,
                    },
                    rgba(148, 163, 184, 0.78),
                );
            }
        }
    }

    // 2. Message list viewport
    let msg_area = D2D_RECT_F {
        left: b.left + pad_x,
        top: b.top + top_bar_h + 6.0,
        right: b.right - pad_x - if input.active { 8.0 } else { 0.0 },
        bottom: b.bottom - input_bar_h - 6.0,
    };
    let msg_w = (msg_area.right - msg_area.left).max(40.0);
    let msg_h = (msg_area.bottom - msg_area.top).max(20.0);

    let line_h = (font_size * 1.78).max(24.0);
    let visible_indices: Vec<usize> = input
        .messages
        .iter()
        .enumerate()
        .filter(|(idx, _)| {
            input.active
                || input.adjust_mode
                || input.settings.always_visible
                || input.opacities.get(*idx).copied().unwrap_or(1.0) > 0.01
        })
        .map(|(idx, _)| idx)
        .collect();

    let mut clamped_scroll = 0.0f32;
    let sender_fmt = gfx.create_text_format(
        font_size,
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
        false,
    );
    let body_fmt = gfx.create_text_format(
        font_size,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
        false,
    );
    let pill_fmt = gfx.create_text_format(
        (font_size - 2.0).max(10.0),
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
        false,
    );

    enum InlineItem {
        Sender(String, (u8, u8, u8), f32),
        Plain(String, f32),
        Url { display: String, copy_value: String, w: f32 },
        RoomCode { code: String, w: f32 },
    }

    if let (Ok(sender_fmt), Ok(body_fmt), Ok(pill_fmt)) = (sender_fmt, body_fmt, pill_fmt) {
        let mut laid_out: Vec<(usize, Vec<Vec<InlineItem>>)> = Vec::with_capacity(visible_indices.len());
        for &idx in &visible_indices {
            let msg = &input.messages[idx];
            let rgb = Timeline::sender_color_rgb(&msg.sender_id);
            let prefix = format!("{}：", msg.sender_name);
            let (pw, _) = gfx.measure_text(&prefix, &sender_fmt, msg_w * 0.45);
            let sender_w = pw.min(msg_w * 0.45) + 2.0;

            let mut rows: Vec<Vec<InlineItem>> = vec![vec![InlineItem::Sender(prefix, rgb, sender_w)]];
            let mut row_x = sender_w;
            let cont_indent = 6.0;

            for seg in parse_segments(&msg.text) {
                match seg {
                    TextSegment::Plain(s) => {
                        let mut rem = s.as_str();
                        while !rem.is_empty() {
                            let avail = (msg_w - row_x).max(24.0);
                            let (full_w, _) = gfx.measure_text(rem, &body_fmt, 8192.0);
                            if full_w <= avail {
                                rows.last_mut().unwrap().push(InlineItem::Plain(rem.to_string(), full_w + 4.0));
                                row_x += full_w + 4.0;
                                break;
                            }
                            // Binary search largest char boundary fitting `avail`
                            let chars: Vec<(usize, char)> = rem.char_indices().collect();
                            let mut lo = 1usize;
                            let mut hi = chars.len();
                            let mut best = 1usize;
                            while lo <= hi {
                                let mid = (lo + hi) / 2;
                                let end_b = if mid < chars.len() { chars[mid].0 } else { rem.len() };
                                let (cw, _) = gfx.measure_text(&rem[..end_b], &body_fmt, 8192.0);
                                if cw <= avail {
                                    best = mid;
                                    lo = mid + 1;
                                } else if mid == 1 {
                                    break;
                                } else {
                                    hi = mid - 1;
                                }
                            }
                            let split_b = if best < chars.len() { chars[best].0 } else { rem.len() };
                            if best <= 1 && row_x > cont_indent + 4.0 {
                                rows.push(Vec::new());
                                row_x = cont_indent;
                                continue;
                            }
                            let chunk = &rem[..split_b];
                            let (cw, _) = gfx.measure_text(chunk, &body_fmt, 8192.0);
                            rows.last_mut().unwrap().push(InlineItem::Plain(chunk.to_string(), cw + 2.0));
                            rem = rem[split_b..].trim_start();
                            if !rem.is_empty() {
                                rows.push(Vec::new());
                                row_x = cont_indent;
                            } else {
                                row_x += cw + 2.0;
                            }
                        }
                    }
                    TextSegment::Url { display, copy_value } => {
                        let label = format!("🔗 {display}");
                        let (tw, _) = gfx.measure_text(&label, &pill_fmt, (msg_w - 28.0).max(40.0));
                        let pill_w = (tw + 20.0).min(msg_w - cont_indent);
                        if row_x + pill_w > msg_w && row_x > cont_indent + 4.0 {
                            rows.push(Vec::new());
                            row_x = cont_indent;
                        }
                        let fit_w = pill_w.min((msg_w - row_x).max(48.0));
                        rows.last_mut().unwrap().push(InlineItem::Url { display, copy_value, w: fit_w });
                        row_x += fit_w + 6.0;
                    }
                    TextSegment::RoomCode { code } => {
                        let label = format!("# {code}");
                        let (tw, _) = gfx.measure_text(&label, &pill_fmt, (msg_w - 28.0).max(40.0));
                        let pill_w = (tw + 18.0).min(msg_w - cont_indent);
                        if row_x + pill_w > msg_w && row_x > cont_indent + 4.0 {
                            rows.push(Vec::new());
                            row_x = cont_indent;
                        }
                        let fit_w = pill_w.min((msg_w - row_x).max(40.0));
                        rows.last_mut().unwrap().push(InlineItem::RoomCode { code, w: fit_w });
                        row_x += fit_w + 6.0;
                    }
                }
            }
            laid_out.push((idx, rows));
        }

        let total_rows: usize = laid_out.iter().map(|(_, r)| r.len()).sum();
        let total_content_h = (total_rows as f32) * line_h;
        let max_scroll = (total_content_h - msg_h).max(0.0);
        hits.max_scroll_px = max_scroll;
        clamped_scroll = if input.active {
            input.scroll_offset_px.clamp(0.0, max_scroll)
        } else {
            0.0
        };

        unsafe {
            dc.PushAxisAlignedClip(
                &D2D_RECT_F {
                    left: b.left,
                    top: msg_area.top,
                    right: b.right,
                    bottom: msg_area.bottom,
                },
                windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            );
        }

        let mut cursor_y = msg_area.bottom + clamped_scroll;
        for (idx, rows) in laid_out.into_iter().rev() {
            let block_h = (rows.len() as f32) * line_h;
            let block_top = cursor_y - block_h;
            if cursor_y < msg_area.top {
                break;
            }
            if block_top <= msg_area.bottom {
                let item_fade = if input.active || input.adjust_mode || input.settings.always_visible {
                    1.0
                } else {
                    input.opacities.get(idx).copied().unwrap_or(1.0)
                };
                let alpha = (item_fade * text_alpha_scale).clamp(0.0, 1.0);

                for (r_idx, row) in rows.into_iter().enumerate() {
                    let ry = block_top + (r_idx as f32) * line_h;
                    if ry + line_h < msg_area.top || ry > msg_area.bottom {
                        continue;
                    }
                    let mut cur_x = msg_area.left + if r_idx > 0 { 6.0 } else { 0.0 };
                    for item in row {
                        match item {
                            InlineItem::Sender(prefix, (sr, sg, sb), sw) => {
                                draw_text_outlined(
                                    dc,
                                    gfx,
                                    &prefix,
                                    &sender_fmt,
                                    D2D_RECT_F {
                                        left: cur_x,
                                        top: ry,
                                        right: cur_x + sw + 4.0,
                                        bottom: ry + line_h,
                                    },
                                    rgba(sr, sg, sb, alpha),
                                    alpha,
                                );
                                cur_x += sw;
                            }
                            InlineItem::Plain(s, sw) => {
                                draw_text_outlined(
                                    dc,
                                    gfx,
                                    &s,
                                    &body_fmt,
                                    D2D_RECT_F {
                                        left: cur_x,
                                        top: ry,
                                        right: (cur_x + sw + 4.0).min(msg_area.right),
                                        bottom: ry + line_h,
                                    },
                                    rgba(255, 255, 255, alpha),
                                    alpha,
                                );
                                cur_x += sw;
                            }
                            InlineItem::Url { display, copy_value, w: pw } => {
                                let label = format!("🔗 {display}");
                                let pill_rect = D2D_RECT_F {
                                    left: cur_x,
                                    top: ry + 2.5,
                                    right: cur_x + pw,
                                    bottom: ry + line_h - 2.5,
                                };
                                fill_round_rect(dc, pill_rect, 6.0, rgba(24, 28, 36, 0.88 * alpha));
                                stroke_round_rect(
                                    dc,
                                    pill_rect,
                                    6.0,
                                    rgba(148, 163, 184, 0.34 * alpha),
                                    1.0,
                                );
                                draw_text_simple(
                                    dc,
                                    gfx,
                                    &label,
                                    &pill_fmt,
                                    D2D_RECT_F {
                                        left: pill_rect.left + 8.0,
                                        top: pill_rect.top,
                                        right: pill_rect.right - 6.0,
                                        bottom: pill_rect.bottom,
                                    },
                                    rgba(226, 232, 240, alpha),
                                );
                                hits.capsules.push(CapsuleHitTarget {
                                    rect: pill_rect,
                                    copy_value,
                                });
                                cur_x += pw + 6.0;
                            }
                            InlineItem::RoomCode { code, w: pw } => {
                                let label = format!("# {code}");
                                let pill_rect = D2D_RECT_F {
                                    left: cur_x,
                                    top: ry + 2.5,
                                    right: cur_x + pw,
                                    bottom: ry + line_h - 2.5,
                                };
                                fill_round_rect(dc, pill_rect, 6.0, rgba(36, 30, 18, 0.90 * alpha));
                                stroke_round_rect(
                                    dc,
                                    pill_rect,
                                    6.0,
                                    rgba(245, 158, 11, 0.45 * alpha),
                                    1.0,
                                );
                                draw_text_simple(
                                    dc,
                                    gfx,
                                    &label,
                                    &pill_fmt,
                                    D2D_RECT_F {
                                        left: pill_rect.left + 8.0,
                                        top: pill_rect.top,
                                        right: pill_rect.right - 6.0,
                                        bottom: pill_rect.bottom,
                                    },
                                    rgba(251, 191, 36, alpha),
                                );
                                hits.capsules.push(CapsuleHitTarget {
                                    rect: pill_rect,
                                    copy_value: code.clone(),
                                });
                                cur_x += pw + 6.0;
                            }
                        }
                    }
                }
            }
            cursor_y = block_top;
        }

        unsafe {
            dc.PopAxisAlignedClip();
        }

        // 3. Slim scrollbar in active mode when scrollable
        if input.active && max_scroll > 1.0 {
            let track_h = msg_h;
            let thumb_h = ((msg_h / total_content_h) * track_h).clamp(24.0, track_h);
            let ratio = 1.0 - (clamped_scroll / max_scroll);
            let thumb_top = msg_area.top + ratio * (track_h - thumb_h);
            let sb_rect = D2D_RECT_F {
                left: b.right - 6.0,
                top: thumb_top,
                right: b.right - 2.5,
                bottom: thumb_top + thumb_h,
            };
            fill_round_rect(dc, sb_rect, 2.0, rgba(148, 163, 184, 0.45));
        }
    }

    // 4. Floating "↓ N 条新消息" pill when scrolled up with unread messages
    if input.active && clamped_scroll > 4.0 && input.unread_count > 0 {
        let pill_text = format!("↓ {} 条新消息", input.unread_count);
        if let Ok(fmt) = gfx.create_text_format(
            11.5,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_CENTER,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            false,
        ) {
            let (tw, _) = gfx.measure_text(&pill_text, &fmt, 160.0);
            let pw = tw + 22.0;
            let pill = D2D_RECT_F {
                left: b.right - pw - 12.0,
                top: b.bottom - input_bar_h - 32.0,
                right: b.right - 12.0,
                bottom: b.bottom - input_bar_h - 8.0,
            };
            fill_round_rect(dc, pill, 12.0, rgba(59, 130, 246, 0.94));
            draw_text_simple(dc, gfx, &pill_text, &fmt, pill, rgba(255, 255, 255, 1.0));
            hits.unread_pill_rect = Some(pill);
        }
    }

    // 5. Transient copied/send toast in passive mode only (active mode renders it inside the top bar;
    //    connection notice_text is NEVER rendered over message rows or in passive mode)
    if !input.active && !input.adjust_mode {
        if let Some(toast) = input.copied_toast {
            if let Ok(fmt) = gfx.create_text_format(
                10.5,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                false,
            ) {
                let max_w = (w - 24.0).max(60.0);
                let (tw, _) = gfx.measure_text(toast, &fmt, max_w - 16.0);
                let pw = (tw + 16.0).min(max_w);
                let pill = D2D_RECT_F {
                    left: b.right - pw - 10.0,
                    top: b.top + 2.0,
                    right: b.right - 10.0,
                    bottom: b.top + 22.0,
                };
                fill_round_rect(dc, pill, 10.0, rgba(16, 20, 28, 0.86));
                stroke_round_rect(dc, pill, 10.0, rgba(148, 163, 184, 0.35), 1.0);
                draw_text_simple(dc, gfx, toast, &fmt, pill, rgba(241, 245, 249, 0.96));
            }
        }
    }

    // 6. Integrated bottom input bar (Active mode)
    if input.active {
        let inp = D2D_RECT_F {
            left: b.left + 8.0,
            top: b.bottom - input_bar_h + 4.0,
            right: b.right - 8.0,
            bottom: b.bottom - 6.0,
        };
        hits.input_rect = inp;
        fill_round_rect(dc, inp, 8.0, rgba(10, 12, 17, 0.72));
        stroke_round_rect(dc, inp, 8.0, rgba(255, 255, 255, 0.12), 1.0);

        let prefix = if input.channel_label.is_empty() {
            "#".to_string()
        } else {
            format!("#{}", input.channel_label.trim_start_matches('#'))
        };

        if let (Ok(pfmt), Ok(ifmt)) = (
            gfx.create_text_format(
                12.5,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                false,
            ),
            gfx.create_text_format(
                13.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                false,
            ),
        ) {
            let (pw, _) = gfx.measure_text(&prefix, &pfmt, 120.0);
            let prefix_w = pw.min(110.0);
            draw_text_simple(
                dc,
                gfx,
                &prefix,
                &pfmt,
                D2D_RECT_F {
                    left: inp.left + 10.0,
                    top: inp.top,
                    right: inp.left + 10.0 + prefix_w + 4.0,
                    bottom: inp.bottom,
                },
                rgba(148, 163, 184, 0.85),
            );

            let text_left = inp.left + 10.0 + prefix_w + 8.0;
            let text_right = inp.right - 28.0;
            let text_w = (text_right - text_left).max(20.0);

            if input.draft.is_empty() && input.ime_composition.is_empty() {
                draw_text_simple(
                    dc,
                    gfx,
                    "输入消息并回车发送…",
                    &ifmt,
                    D2D_RECT_F {
                        left: text_left,
                        top: inp.top,
                        right: text_right,
                        bottom: inp.bottom,
                    },
                    rgba(148, 163, 184, 0.50),
                );
                hits.caret_pos = (text_left, inp.bottom);
            } else {
                let (dw, _) = gfx.measure_text(input.draft, &ifmt, text_w);
                draw_text_simple(
                    dc,
                    gfx,
                    input.draft,
                    &ifmt,
                    D2D_RECT_F {
                        left: text_left,
                        top: inp.top,
                        right: text_right,
                        bottom: inp.bottom,
                    },
                    rgba(248, 250, 252, 0.98),
                );
                let mut caret_x = (text_left + dw).min(text_right - 2.0);

                // IME pre-edit composition text with underline
                if !input.ime_composition.is_empty() {
                    let (cw, _) = gfx.measure_text(input.ime_composition, &ifmt, text_right - caret_x);
                    draw_text_simple(
                        dc,
                        gfx,
                        input.ime_composition,
                        &ifmt,
                        D2D_RECT_F {
                            left: caret_x,
                            top: inp.top,
                            right: text_right,
                            bottom: inp.bottom,
                        },
                        rgba(147, 197, 253, 1.0),
                    );
                    if let Ok(ul) = brush(dc, rgba(96, 165, 250, 0.9)) {
                        unsafe {
                            dc.DrawLine(
                                D2D_POINT_2F {
                                    x: caret_x,
                                    y: inp.bottom - 5.0,
                                },
                                D2D_POINT_2F {
                                    x: (caret_x + cw).min(text_right),
                                    y: inp.bottom - 5.0,
                                },
                                &ul,
                                1.2,
                                None,
                            );
                        }
                    }
                    caret_x = (caret_x + cw).min(text_right - 2.0);
                }

                hits.caret_pos = (caret_x, inp.bottom);
            }

            // Caret line
            if input.caret_visible {
                if let Ok(cb) = brush(dc, rgba(248, 250, 252, 0.95)) {
                    unsafe {
                        dc.DrawLine(
                            D2D_POINT_2F {
                                x: hits.caret_pos.0 + 1.0,
                                y: inp.top + 6.0,
                            },
                            D2D_POINT_2F {
                                x: hits.caret_pos.0 + 1.0,
                                y: inp.bottom - 6.0,
                            },
                            &cb,
                            1.5,
                            None,
                        );
                    }
                }
            }

            // Enter hint glyph `↵`
            draw_text_simple(
                dc,
                gfx,
                "↵",
                &pfmt,
                D2D_RECT_F {
                    left: inp.right - 22.0,
                    top: inp.top,
                    right: inp.right - 6.0,
                    bottom: inp.bottom,
                },
                rgba(148, 163, 184, 0.7),
            );
        }
    }

    // 7. Bottom-right resize grip in active / adjust mode
    if input.active || input.adjust_mode {
        hits.resize_grip_rect = D2D_RECT_F {
            left: b.right - 16.0,
            top: b.bottom - 16.0,
            right: b.right,
            bottom: b.bottom,
        };
        if let Ok(gb) = brush(dc, rgba(148, 163, 184, 0.45)) {
            unsafe {
                for offset in [4.0f32, 8.0] {
                    dc.DrawLine(
                        D2D_POINT_2F {
                            x: b.right - offset - 3.0,
                            y: b.bottom - 3.0,
                        },
                        D2D_POINT_2F {
                            x: b.right - 3.0,
                            y: b.bottom - offset - 3.0,
                        },
                        &gb,
                        1.0,
                        None,
                    );
                }
            }
        }
    }

    // 8. Quick-switch text channel dropdown menu overlay
    if input.active && input.channel_menu_open && !input.channels.is_empty() {
        let item_h = 28.0;
        let max_items = input.channels.len().min(6);
        let menu_w = 190.0f32.min(w - 28.0);
        let menu_h = (max_items as f32) * item_h + 8.0;
        let menu_rect = D2D_RECT_F {
            left: b.left + 16.0,
            top: b.top + top_bar_h + 2.0,
            right: b.left + 16.0 + menu_w,
            bottom: (b.top + top_bar_h + 2.0 + menu_h).min(b.bottom - input_bar_h - 4.0),
        };
        fill_round_rect(dc, menu_rect, 8.0, rgba(15, 20, 29, 0.96));
        stroke_round_rect(dc, menu_rect, 8.0, rgba(255, 255, 255, 0.14), 1.0);

        if let Ok(mfmt) = gfx.create_text_format(
            12.5,
            DWRITE_FONT_WEIGHT_MEDIUM,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            false,
        ) {
            for (i, ch) in input.channels.iter().take(max_items).enumerate() {
                let row = D2D_RECT_F {
                    left: menu_rect.left + 4.0,
                    top: menu_rect.top + 4.0 + (i as f32) * item_h,
                    right: menu_rect.right - 4.0,
                    bottom: menu_rect.top + 4.0 + ((i + 1) as f32) * item_h,
                };
                let is_selected = ch.channel_id == input.settings.channel_id;
                let is_hovered = input.hovered_channel_idx == Some(i);
                if is_selected {
                    fill_round_rect(dc, row, 6.0, rgba(59, 130, 246, 0.28));
                } else if is_hovered {
                    fill_round_rect(dc, row, 6.0, rgba(255, 255, 255, 0.08));
                }
                let label = format!("# {}", ch.channel_name);
                draw_text_simple(
                    dc,
                    gfx,
                    &label,
                    &mfmt,
                    D2D_RECT_F {
                        left: row.left + 8.0,
                        top: row.top,
                        right: row.right - 8.0,
                        bottom: row.bottom,
                    },
                    if is_selected {
                        rgba(147, 197, 253, 1.0)
                    } else {
                        rgba(226, 232, 240, 0.92)
                    },
                );
                hits.channel_menu_items.push(ChannelMenuHitTarget {
                    rect: row,
                    channel_id: ch.channel_id.clone(),
                    channel_name: ch.channel_name.clone(),
                });
            }
        }
    }

    unsafe {
        dc.PopAxisAlignedClip();
    }
    hits
}

fn draw_passive_radial_backdrop(
    dc: &ID2D1DeviceContext,
    b: D2D_RECT_F,
    corner: DockCorner,
    alpha: f32,
) {
    let w = (b.right - b.left).max(1.0);
    let h = (b.bottom - b.top).max(1.0);
    let center = match corner {
        DockCorner::TopLeft => D2D_POINT_2F {
            x: b.left + w * 0.42,
            y: b.top + h * 0.42,
        },
        DockCorner::TopRight => D2D_POINT_2F {
            x: b.left + w * 0.58,
            y: b.top + h * 0.42,
        },
        DockCorner::BottomLeft => D2D_POINT_2F {
            x: b.left + w * 0.44,
            y: b.top + h * 0.58,
        },
        DockCorner::BottomRight => D2D_POINT_2F {
            x: b.left + w * 0.56,
            y: b.top + h * 0.58,
        },
    };
    let rx = (center.x - b.left).min(b.right - center.x).max(1.0);
    let ry = (center.y - b.top).min(b.bottom - center.y).max(1.0);
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: rgba(10, 12, 18, (alpha * 1.15).min(0.85)),
        },
        D2D1_GRADIENT_STOP {
            position: 0.52,
            color: rgba(10, 12, 18, alpha * 0.45),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: rgba(10, 12, 18, 0.0),
        },
    ];
    unsafe {
        let rt: &ID2D1RenderTarget = dc;
        if let Ok(stop_col) =
            rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        {
            let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center,
                gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
                radiusX: rx,
                radiusY: ry,
            };
            if let Ok(rbrush) = rt.CreateRadialGradientBrush(&props, None, &stop_col) {
                dc.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: center,
                        radiusX: rx,
                        radiusY: ry,
                    },
                    &rbrush,
                );
            }
        }
    }
}

/// Sample messages matching the redesign mockup for Live Preview & Adjust mode.
pub fn sample_preview_messages() -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            message_id: "sample-1".into(),
            timestamp_us: 1,
            sender_id: "uid-xiazi-01".into(),
            sender_name: "匣子".into(),
            text: "A 大刚封烟，别急着拉出去".into(),
            mine: false,
        },
        ChatMessage {
            message_id: "sample-2".into(),
            timestamp_us: 2,
            sender_id: "uid-liangyu-02".into(),
            sender_name: "凉雨".into(),
            text: "这把打完看下回放 https://www.bilibili.com/video/BV1xx411c7mD".into(),
            mine: false,
        },
        ChatMessage {
            message_id: "sample-3".into(),
            timestamp_us: 3,
            sender_id: "uid-heiye-03".into(),
            sender_name: "黑夜".into(),
            text: "换自定义房，密码 QILZU3".into(),
            mine: false,
        },
    ]
}
