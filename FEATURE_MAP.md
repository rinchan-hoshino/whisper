# Whisper Feature Map

## Portable Core (`src/core/`)
- `settings.rs`: `AppSettings` (v2.0 schema + lossless 1.x migration), `DockCorner`, default sound & opacity clamping.
- `timeline.rs`: `ChatMessage` & `Timeline` (dedup, 80-message cap, 8s hold + 2s fade-out curve, unread tracking, stable FNV-1a sender color palette).
- `capsules.rs`: Inline micro-capsule parser for URLs (`[🔗 domain/…]`) and 4–10 char alphanumeric room/invite codes (`[# QILZU3]`).
- `hotkey.rs`: Win32 virtual-key hotkey parser & formatter (`F1`–`F12`, modifiers + `A-Z` / `0-9` / named keys).
- `protocol.rs`: Oopz REST `Oopz-Sign` (MD5 + RSA-PKCS1v15-SHA256 + Base64), local login registry payload parser (with JWT `exp` check), WS frame builders (`253`, `249`, `254`) and event parser (`21`, `9`, `57`, `8`).
- `updater_core.rs`: Manifest parser, semver comparison, `https://acv.k-neco.com/` origin enforcement, SHA-256 + byte-length verification.

## Windows Native Platform (`src/platform/`)
- `d2d.rs`: Shared D3D11 (hardware + WARP fallback) / DXGI / DirectComposition / Direct2D / DirectWrite graphics engine & per-window `DCompTarget`.
- `hud_render.rs`: 1:1 Direct2D HUD renderer shared by both the live HUD window and Settings Live Preview stage.
- `hud_window.rs`: `WS_EX_NOREDIRECTIONBITMAP | TOOLWINDOW | TOPMOST | NOACTIVATE | TRANSPARENT` HUD window, `AttachThreadInput + SetForegroundWindow` focus handoff, IMM32 pre-edit & candidate window positioning, clipboard copy.
- `settings_window.rs`: Native Direct2D `Whisper 设置` window (1:1 Live Preview with Bright Sand / Dark Room + screen drag-position mode, Area & Text Channel dropdowns, click-to-record hotkeys, 4-quadrant dock corner selector, sliders, toggles, Windows system sound selector + `▶ 试听`, live memory footer & update button).
- `app.rs`: Single-threaded Win32 message loop, global hotkey registration, on-demand dirty redraw (0% idle CPU/GPU), drag/resize with `Esc` cancel.
- `winhttp.rs` & `gateway.rs`: Zero-bloat WinHTTP REST + WebSocket client and background Oopz gateway worker with exponential backoff reconnect.
- `dpapi.rs`: DPAPI `CryptProtectData` / `CryptUnprotectData` persistence (`%LOCALAPPDATA%\RinChan\Whisper\settings.dat`) + automatic 1.x migration from `%LOCALAPPDATA%\RinChan\OopzTacticalOverlay\state.bin`.
- `session.rs`: Local Oopz process & `HKCU\Software\Oopz\OopzData` session discovery.
- `sound.rs`: `C:\Windows\Media` (incl. 1-level theme subdirs) `.wav` enumerator, builtin 64ms synthesized tap (`__builtin_tap__`), `PlaySoundW` preview & non-interrupting live playback.
- `updater.rs`: Verified download + atomic `.exe -> .exe.old` self-replacement + startup `.old` cleanup.
- `tray.rs`, `autostart.rs`, `single_instance.rs`: System tray menu, `HKCU\...\Run` autostart, single-instance mutex activation.
