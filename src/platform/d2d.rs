use std::ffi::c_void;
use windows::core::{implement, IUnknown, Interface, PCWSTR};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::{BOOL, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory, ID2D1Factory1,
    ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_LINE_JOIN_ROUND, D2D1_ROUNDED_RECT,
    D2D1_STROKE_STYLE_PROPERTIES, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
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
    DWriteCreateFactory, IDWriteFactory, IDWriteInlineObject, IDWritePixelSnapping_Impl,
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTextRenderer, IDWriteTextRenderer_Impl,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_GLYPH_RUN, DWRITE_GLYPH_RUN_DESCRIPTION, DWRITE_MATRIX,
    DWRITE_MEASURING_MODE, DWRITE_PARAGRAPH_ALIGNMENT, DWRITE_STRIKETHROUGH,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_METRICS, DWRITE_UNDERLINE, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_WORD_WRAPPING_WRAP,
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
    pub d2d_factory: ID2D1Factory1,
    pub d2d_device: ID2D1Device,
    pub dwrite: IDWriteFactory,
    pub dcomp: IDCompositionDevice,
    pub round_stroke_style: ID2D1StrokeStyle,
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
            let stroke_props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 1.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
            };
            let round_stroke_style = ID2D1Factory::CreateStrokeStyle(&d2d_factory, &stroke_props, None)
                .map_err(|e| format!("CreateStrokeStyle failed: {e}"))?;

            Ok(Self {
                _d3d_device: d3d_device,
                dxgi_device,
                dxgi_factory,
                d2d_factory,
                d2d_device,
                dwrite,
                dcomp,
                round_stroke_style,
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

#[implement(IDWriteTextRenderer)]
struct OutlineTextRenderer {
    d2d_factory: ID2D1Factory1,
    dc: ID2D1DeviceContext,
    outline_brush: ID2D1SolidColorBrush,
    fg_brush: ID2D1SolidColorBrush,
    stroke_style: ID2D1StrokeStyle,
    draw_outline: bool,
}

impl IDWritePixelSnapping_Impl for OutlineTextRenderer_Impl {
    fn IsPixelSnappingDisabled(&self, _clientdrawingcontext: *const c_void) -> windows::core::Result<BOOL> {
        Ok(BOOL(0))
    }

    fn GetCurrentTransform(
        &self,
        _clientdrawingcontext: *const c_void,
        transform: *mut DWRITE_MATRIX,
    ) -> windows::core::Result<()> {
        if !transform.is_null() {
            unsafe {
                *transform = DWRITE_MATRIX {
                    m11: 1.0,
                    m12: 0.0,
                    m21: 0.0,
                    m22: 1.0,
                    dx: 0.0,
                    dy: 0.0,
                };
            }
        }
        Ok(())
    }

    fn GetPixelsPerDip(&self, _clientdrawingcontext: *const c_void) -> windows::core::Result<f32> {
        let mut dpi_x = 96.0f32;
        let mut dpi_y = 96.0f32;
        unsafe {
            self.dc.GetDpi(&mut dpi_x, &mut dpi_y);
        }
        Ok((dpi_x / 96.0).max(1.0))
    }
}

impl IDWriteTextRenderer_Impl for OutlineTextRenderer_Impl {
    fn DrawGlyphRun(
        &self,
        _clientdrawingcontext: *const c_void,
        baselineoriginx: f32,
        baselineoriginy: f32,
        measuringmode: DWRITE_MEASURING_MODE,
        glyphrun: *const DWRITE_GLYPH_RUN,
        _glyphrundescription: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        unsafe {
            let Some(run) = glyphrun.as_ref() else {
                return Ok(());
            };
            if run.glyphCount == 0 {
                return Ok(());
            }
            let Some(font_face) = run.fontFace.as_ref() else {
                return Ok(());
            };

            let path_geom = self.d2d_factory.CreatePathGeometry()?;
            let sink = path_geom.Open()?;
            let outline_res = font_face.GetGlyphRunOutline(
                run.fontEmSize,
                run.glyphIndices,
                Some(run.glyphAdvances),
                Some(run.glyphOffsets),
                run.glyphCount,
                run.isSideways.as_bool(),
                (run.bidiLevel & 1) != 0,
                &sink,
            );
            let _ = sink.Close();

            if outline_res.is_ok() {
                let translation = Matrix3x2::translation(baselineoriginx, baselineoriginy);
                let translated = self
                    .d2d_factory
                    .CreateTransformedGeometry(&path_geom, &translation)?;
                if self.draw_outline {
                    let stroke_width = (run.fontEmSize * 0.17).clamp(2.3, 3.4);
                    self.dc.DrawGeometry(
                        &translated,
                        &self.outline_brush,
                        stroke_width,
                        &self.stroke_style,
                    );
                }
                self.dc.FillGeometry(&translated, &self.fg_brush, None);
            } else {
                // Fallback for non-outline bitmap/color glyphs
                ID2D1RenderTarget::DrawGlyphRun(
                    &self.dc,
                    D2D_POINT_2F {
                        x: baselineoriginx,
                        y: baselineoriginy,
                    },
                    run,
                    &self.fg_brush,
                    measuringmode,
                );
            }
            Ok(())
        }
    }

    fn DrawUnderline(
        &self,
        _clientdrawingcontext: *const c_void,
        _baselineoriginx: f32,
        _baselineoriginy: f32,
        _underline: *const DWRITE_UNDERLINE,
        _clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn DrawStrikethrough(
        &self,
        _clientdrawingcontext: *const c_void,
        _baselineoriginx: f32,
        _baselineoriginy: f32,
        _strikethrough: *const DWRITE_STRIKETHROUGH,
        _clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn DrawInlineObject(
        &self,
        _clientdrawingcontext: *const c_void,
        _originx: f32,
        _originy: f32,
        _inlineobject: Option<&IDWriteInlineObject>,
        _issideways: BOOL,
        _isrighttoleft: BOOL,
        _clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Crisp game-HUD text with true DirectWrite vector glyph outline (360-degree uniform round stroke)
/// so white/colored text stays razor-sharp even on pure white snow/sand backgrounds.
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
    let Ok(fg_brush) = brush(dc, color) else {
        return;
    };
    let outline_alpha = (0.92 * shadow_alpha).clamp(0.0, 1.0);
    let Ok(outline_brush) = brush(dc, rgba(8, 10, 14, outline_alpha)) else {
        return;
    };

    let renderer: IDWriteTextRenderer = OutlineTextRenderer {
        d2d_factory: gfx.d2d_factory.clone(),
        dc: dc.clone(),
        outline_brush,
        fg_brush,
        stroke_style: gfx.round_stroke_style.clone(),
        draw_outline: shadow_alpha > 0.01,
    }
    .into();

    let clip = D2D_RECT_F {
        left: rect.left - 2.5,
        top: rect.top - 2.5,
        right: rect.right + 2.5,
        bottom: rect.bottom + 2.5,
    };
    unsafe {
        dc.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        let _ = layout.Draw(None, &renderer, rect.left, rect.top);
        dc.PopAxisAlignedClip();
    }
}
