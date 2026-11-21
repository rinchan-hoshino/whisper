pub mod app;
pub mod autostart;
pub mod d2d;
pub mod dpapi;
pub mod gateway;
pub mod hud_render;
pub mod hud_window;
pub mod session;
pub mod settings_window;
pub mod single_instance;
pub mod sound;
pub mod tray;
pub mod updater;
pub mod winhttp;

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

pub fn to_wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
