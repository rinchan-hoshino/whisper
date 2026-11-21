use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1,
    ID2D1SolidColorBrush, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_PARAGRAPH_ALIGNMENT, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_METRICS,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1, DXGI_PRESENT,
    DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

use crate::platform::to_wide_null;

#[inline]
pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: (r as f32) / 255.0,
        g: (g as f32) / 255.0,
        b: (b as f32) / 255.0,
        a,
    }
}

pub struct GraphicsEngine {
    pub _d3d_device: ID3D11Device,
    pub dxgi_device: IDXGIDevice,
    pub dxgi_factory: IDXGIFactory2,
    pub _d2d_factory: ID2D1Factory1,
    pub d2d_device: ID2D1Device,
    pub dwrite: IDWriteFactory,
    pub dcomp: IDCompositionDevice,
}

impl GraphicsEngine {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let feature_levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
            let mut d3d_device: Option<ID3D11Device> = None;
            let hr = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                None,
                None,
            );
            if hr.is_err() || d3d_device.is_none() {
                D3D11CreateDevice(
                    None,
                    D3D_DRIVER_TYPE_WARP,
                    None,
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    Some(&feature_levels),
                    D3D11_SDK_VERSION,
                    Some(&mut d3d_device),
                    None,
                    None,
                )
                .map_err(|e| format!("D3D11CreateDevice (WARP fallback) failed: {e}"))?;
            }
            let d3d_device = d3d_device.ok_or("D3D11 device missing")?;
            let dxgi_device: IDXGIDevice = d3d_device
                .cast()
                .map_err(|e| format!("cast IDXGIDevice failed: {e}"))?;
            let adapter = dxgi_device
                .GetAdapter()
                .map_err(|e| format!("GetAdapter failed: {e}"))?;
            let dxgi_factory: IDXGIFactory2 = adapter
                .GetParent()
                .map_err(|e| format!("GetParent IDXGIFactory2 failed: {e}"))?;

            let d2d_factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                    .map_err(|e| format!("D2D1CreateFactory failed: {e}"))?;
            let d2d_device = d2d_factory
                .CreateDevice(&dxgi_device)
                .map_err(|e| format!("CreateDevice D2D failed: {e}"))?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
                .map_err(|e| format!("DWriteCreateFactory failed: {e}"))?;
            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)
                .map_err(|e| format!("DCompositionCreateDevice failed: {e}"))?;

            Ok(Self {
                _d3d_device: d3d_device,
                dxgi_device,
                dxgi_factory,
                _d2d_factory: d2d_factory,
                d2d_device,
                dwrite,
                dcomp,
            })
        }
    }

    pub fn create_text_format(
        &self,
        size: f32,
        weight: DWRITE_FONT_WEIGHT,
        align: DWRITE_TEXT_ALIGNMENT,
        valign: DWRITE_PARAGRAPH_ALIGNMENT,
        wrap: bool,
    ) -> Result<IDWriteTextFormat, String> {
        let family = to_wide_null("Microsoft YaHei UI");
        let locale = to_wide_null("zh-CN");
        unsafe {
            let fmt = self
                .dwrite
                .CreateTextFormat(
                    PCWSTR(family.as_ptr()),
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    PCWSTR(locale.as_ptr()),
                )
                .map_err(|e| format!("CreateTextFormat failed: {e}"))?;
            let _ = fmt.SetTextAlignment(align);
            let _ = fmt.SetParagraphAlignment(valign);
            let _ = fmt.SetWordWrapping(if wrap {
                DWRITE_WORD_WRAPPING_WRAP
            } else {
                DWRITE_WORD_WRAPPING_NO_WRAP
            });
            if !wrap {
                if let Ok(ellipsis) = self.dwrite.CreateEllipsisTrimmingSign(&fmt) {
                    let trim = windows::Win32::Graphics::DirectWrite::DWRITE_TRIMMING {
                        granularity: windows::Win32::Graphics::DirectWrite::DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                        delimiter: 0,
                        delimiterCount: 0,
                    };
                    let _ = fmt.SetTrimming(&trim, &ellipsis);
                }
            }
            Ok(fmt)
        }
    }

    pub fn create_text_layout(
        &self,
        text: &str,
        format: &IDWriteTextFormat,
        max_w: f32,
        max_h: f32,
    ) -> Result<IDWriteTextLayout, String> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.dwrite
                .CreateTextLayout(&wide, format, max_w.max(1.0), max_h.max(1.0))
                .map_err(|e| format!("CreateTextLayout failed: {e}"))
        }
    }

    pub fn measure_text(
        &self,
        text: &str,
        format: &IDWriteTextFormat,
        max_w: f32,
    ) -> (f32, f32) {
        if let Ok(layout) = self.create_text_layout(text, format, max_w, 4096.0) {
            let mut metrics = DWRITE_TEXT_METRICS::default();
            if unsafe { layout.GetMetrics(&mut metrics) }.is_ok() {
                return (metrics.widthIncludingTrailingWhitespace, metrics.height);
            }
        }
        (0.0, 0.0)
    }
}

pub struct DCompTarget {
    pub dc: ID2D1DeviceContext,
    pub swap_chain: IDXGISwapChain1,
    pub _target: IDCompositionTarget,
    pub _visual: IDCompositionVisual,
    pub bitmap: Option<ID2D1Bitmap1>,
    pub width_px: u32,
    pub height_px: u32,
    pub dpi: f32,
}

impl DCompTarget {
    pub fn new(
        gfx: &GraphicsEngine,
        hwnd: HWND,
        width_px: u32,
        height_px: u32,
        dpi: f32,
    ) -> Result<Self, String> {
        let w = width_px.max(1);
        let h = height_px.max(1);
        unsafe {
            let dc = gfx
                .d2d_device
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)
                .map_err(|e| format!("CreateDeviceContext failed: {e}"))?;
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            dc.SetDpi(dpi, dpi);

            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: w,
                Height: h,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                Flags: 0,
            };

            let swap_chain = gfx
                .dxgi_factory
                .CreateSwapChainForComposition(&gfx.dxgi_device, &desc, None)
                .map_err(|e| format!("CreateSwapChainForComposition failed: {e}"))?;

            let target = gfx
                .dcomp
                .CreateTargetForHwnd(hwnd, true)
                .map_err(|e| format!("CreateTargetForHwnd failed: {e}"))?;
            let visual = gfx
                .dcomp
                .CreateVisual()
                .map_err(|e| format!("CreateVisual failed: {e}"))?;
            visual
                .SetContent(&swap_chain)
                .map_err(|e| format!("SetContent failed: {e}"))?;
            target
                .SetRoot(&visual)
                .map_err(|e| format!("SetRoot failed: {e}"))?;
            gfx.dcomp
                .Commit()
                .map_err(|e| format!("DComp Commit failed: {e}"))?;

            let mut out = Self {
                dc,
                swap_chain,
                _target: target,
                _visual: visual,
                bitmap: None,
                width_px: w,
                height_px: h,
                dpi,
            };
            out.bind_backbuffer()?;
            Ok(out)
        }
    }

    fn bind_backbuffer(&mut self) -> Result<(), String> {
        unsafe {
            let surface: IDXGISurface = self
                .swap_chain
                .GetBuffer(0)
                .map_err(|e| format!("GetBuffer(0) failed: {e}"))?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: self.dpi,
                dpiY: self.dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let bmp = self
                .dc
                .CreateBitmapFromDxgiSurface(&surface, Some(&props))
                .map_err(|e| format!("CreateBitmapFromDxgiSurface failed: {e}"))?;
            self.dc.SetTarget(&bmp);
            self.dc.SetDpi(self.dpi, self.dpi);
            self.bitmap = Some(bmp);
            Ok(())
        }
    }

    pub fn resize(&mut self, width_px: u32, height_px: u32, dpi: f32) -> Result<(), String> {
        let w = width_px.max(1);
        let h = height_px.max(1);
        if w == self.width_px && h == self.height_px && (dpi - self.dpi).abs() < 0.1 {
            return Ok(());
        }
        unsafe {
            self.dc.SetTarget(None);
            self.bitmap = None;
            self.swap_chain
                .ResizeBuffers(2, w, h, DXGI_FORMAT_B8G8R8A8_UNORM, Default::default())
                .map_err(|e| format!("ResizeBuffers failed: {e}"))?;
            self.width_px = w;
            self.height_px = h;
            self.dpi = dpi;
            self.bind_backbuffer()
        }
    }

    pub fn dip_size(&self) -> (f32, f32) {
        let scale = self.dpi / 96.0;
        (
            (self.width_px as f32) / scale,
            (self.height_px as f32) / scale,
        )
    }

    pub fn begin_frame(&self) {
        unsafe {
            self.dc.BeginDraw();
            self.dc.Clear(Some(&rgba(0, 0, 0, 0.0)));
        }
    }

    pub fn end_frame(&self, gfx: &GraphicsEngine) {
        unsafe {
            let _ = self.dc.EndDraw(None, None);
            let _ = self.swap_chain.Present(0, DXGI_PRESENT(0));
            let _ = gfx.dcomp.Commit();
        }
    }
}

pub fn brush(dc: &ID2D1DeviceContext, color: D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush, String> {
    unsafe {
        let rt: &windows::Win32::Graphics::Direct2D::ID2D1RenderTarget = dc;
        rt.CreateSolidColorBrush(&color, None)
            .map_err(|e| format!("CreateSolidColorBrush failed: {e}"))
    }
}

pub fn fill_round_rect(
    dc: &ID2D1DeviceContext,
    rect: D2D_RECT_F,
    radius: f32,
    color: D2D1_COLOR_F,
) {
    if let Ok(b) = brush(dc, color) {
        let rr = D2D1_ROUNDED_RECT {
            rect,
            radiusX: radius,
            radiusY: radius,
        };
        unsafe {
            dc.FillRoundedRectangle(&rr, &b);
        }
    }
}

pub fn stroke_round_rect(
    dc: &ID2D1DeviceContext,
    rect: D2D_RECT_F,
    radius: f32,
    color: D2D1_COLOR_F,
    stroke_width: f32,
) {
    if let Ok(b) = brush(dc, color) {
        let rr = D2D1_ROUNDED_RECT {
            rect,
            radiusX: radius,
            radiusY: radius,
        };
        unsafe {
            dc.DrawRoundedRectangle(&rr, &b, stroke_width, None);
        }
    }
}

pub fn draw_text_simple(
    dc: &ID2D1DeviceContext,
    gfx: &GraphicsEngine,
    text: &str,
    fmt: &IDWriteTextFormat,
    rect: D2D_RECT_F,
    color: D2D1_COLOR_F,
) {
    let w = (rect.right - rect.left).max(1.0);
    let h = (rect.bottom - rect.top).max(1.0);
    if let (Ok(layout), Ok(b)) = (gfx.create_text_layout(text, fmt, w, h), brush(dc, color)) {
        unsafe {
            dc.DrawTextLayout(
                D2D_POINT_2F {
                    x: rect.left,
                    y: rect.top,
                },
                &layout,
                &b,
                D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            );
        }
    }
}

/// Crisp game-HUD text with tight dark outline/shadow so white text remains razor-sharp on bright sand.
pub fn draw_text_outlined(
    dc: &ID2D1DeviceContext,
    gfx: &GraphicsEngine,
    text: &str,
    fmt: &IDWriteTextFormat,
    rect: D2D_RECT_F,
    color: D2D1_COLOR_F,
    shadow_alpha: f32,
) {
    let w = (rect.right - rect.left).max(1.0);
    let h = (rect.bottom - rect.top).max(1.0);
    let Ok(layout) = gfx.create_text_layout(text, fmt, w, h) else {
        return;
    };
    if shadow_alpha > 0.01 {
        if let Ok(halo) = brush(dc, rgba(0, 0, 0, (0.44 * shadow_alpha).clamp(0.0, 1.0))) {
            let offsets = [(-0.65, 0.0), (0.65, 0.0), (0.0, -0.65), (0.0, 0.65)];
            unsafe {
                for (dx, dy) in offsets {
                    dc.DrawTextLayout(
                        D2D_POINT_2F {
                            x: rect.left + dx,
                            y: rect.top + dy,
                        },
                        &layout,
                        &halo,
                        D2D1_DRAW_TEXT_OPTIONS_CLIP,
                    );
                }
            }
        }
        if let Ok(drop_b) = brush(dc, rgba(0, 0, 0, (0.64 * shadow_alpha).clamp(0.0, 1.0))) {
            unsafe {
                dc.DrawTextLayout(
                    D2D_POINT_2F {
                        x: rect.left + 0.55,
                        y: rect.top + 1.15,
                    },
                    &layout,
                    &drop_b,
                    D2D1_DRAW_TEXT_OPTIONS_CLIP,
                );
            }
        }
    }
    if let Ok(fg) = brush(dc, color) {
        unsafe {
            dc.DrawTextLayout(
                D2D_POINT_2F {
                    x: rect.left,
                    y: rect.top,
                },
                &layout,
                &fg,
                D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            );
        }
    }
}
