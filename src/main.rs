#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod platform;

use whisper::core;

#[cfg(windows)]
fn main() {
    use std::path::PathBuf;
    use std::process::Command;
    use platform::app::run_app;
    use platform::single_instance::SingleInstanceGuard;
    use platform::updater::{cleanup_explicit_path, cleanup_old_binaries};

    cleanup_old_binaries();

    let args: Vec<String> = std::env::args().collect();

    // Compatibility with legacy 1.0.4 updater handoff
    if let Some(pos) = args.iter().position(|a| a == "--cleanup-updater") {
        if let Some(target) = args.get(pos + 1) {
            cleanup_explicit_path(&PathBuf::from(target));
        }
    }

    let guard = match SingleInstanceGuard::acquire() {
        Some(g) => g,
        None => return,
    };

    let relaunch_exe = match run_app() {
        Ok(path) => path,
        Err(_) => None,
    };

    drop(guard);

    if let Some(exe) = relaunch_exe {
        let _ = Command::new(exe).spawn();
    }
}

#[cfg(not(windows))]
fn main() {
    let _ = core::settings::APP_VERSION;
    eprintln!("Whisper is a native Windows DirectComposition/Direct2D application. Run `cargo test --lib` on macOS or cross-compile with `cargo xwin build --target x86_64-pc-windows-msvc --release`.");
}
