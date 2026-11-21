use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows::core::PCWSTR;
use windows::Win32::Media::Audio::{
    PlaySoundW, SND_ASYNC, SND_FILENAME, SND_MEMORY, SND_NODEFAULT, SND_NOSTOP,
};

use crate::core::settings::BUILTIN_TAP_SOUND;
use crate::platform::to_wide_null;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundOption {
    pub label: String,
    pub key: String,
}

const RECOMMENDED_SOUNDS: [(&str, &str); 4] = [
    ("Windows Navigation Start.wav", "清脆轻点 (Navigation Start)"),
    ("Windows Menu Command.wav", "短促嘀嗒 (Menu Command)"),
    ("Windows Feed Discovered.wav", "轻柔水滴 (Feed Discovered)"),
    ("Windows Information Bar.wav", "柔和提示 (Information Bar)"),
];

fn media_root() -> PathBuf {
    let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    PathBuf::from(sysroot).join("Media")
}

/// Enumerate `C:\Windows\Media` (root + 1-level theme subdirs) for `.wav` files.
/// Recommended short sounds come first, followed by builtin 64ms synthesized tap, then remaining wavs.
pub fn enumerate_sounds() -> Vec<SoundOption> {
    let root = media_root();
    let mut found_rel: Vec<String> = Vec::new();

    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if is_wav(&path) {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        found_rel.push(name.to_string());
                    }
                }
            } else if path.is_dir() {
                if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                    if let Ok(sub_entries) = fs::read_dir(&path) {
                        for sub in sub_entries.flatten() {
                            let sub_path = sub.path();
                            if sub_path.is_file() && is_wav(&sub_path) {
                                if let Some(fname) = sub_path.file_name().and_then(|n| n.to_str()) {
                                    found_rel.push(format!("{dir_name}/{fname}"));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    found_rel.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));

    let mut options: Vec<SoundOption> = Vec::new();
    for (rec_file, rec_label) in RECOMMENDED_SOUNDS {
        if let Some(pos) = found_rel
            .iter()
            .position(|x| x.eq_ignore_ascii_case(rec_file))
        {
            let key = found_rel.remove(pos);
            options.push(SoundOption {
                label: rec_label.to_string(),
                key,
            });
        }
    }

    options.push(SoundOption {
        label: "内置轻触音 (64ms 合成)".to_string(),
        key: BUILTIN_TAP_SOUND.to_string(),
    });

    for rel in found_rel {
        let label = rel
            .strip_suffix(".wav")
            .or_else(|| rel.strip_suffix(".WAV"))
            .unwrap_or(&rel)
            .to_string();
        options.push(SoundOption { label, key: rel });
    }

    options
}

fn is_wav(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("wav"))
        .unwrap_or(false)
}

/// Generate a crisp 64ms 16-bit PCM mono 44.1kHz synthesized tap sound in memory.
fn builtin_tap_wav() -> &'static [u8] {
    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    WAV.get_or_init(|| {
        let sample_rate: u32 = 44100;
        let num_samples: usize = (sample_rate as usize * 64) / 1000;
        let data_bytes = (num_samples * 2) as u32;
        let mut buf = Vec::with_capacity(44 + data_bytes as usize);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
        buf.extend_from_slice(&1u16.to_le_bytes()); // Mono
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // Byte rate
        buf.extend_from_slice(&2u16.to_le_bytes()); // Block align
        buf.extend_from_slice(&16u16.to_le_bytes()); // Bits per sample
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_bytes.to_le_bytes());

        for i in 0..num_samples {
            let t = i as f64 / sample_rate as f64;
            let env = (-t * 68.0).exp() * (1.0 - (-t * 1400.0).exp());
            let tone = (2.0 * std::f64::consts::PI * 1046.5 * t).sin() * 0.65
                + (2.0 * std::f64::consts::PI * 1567.98 * t).sin() * 0.35;
            let sample = (tone * env * 14000.0).clamp(-32767.0, 32767.0) as i16;
            buf.extend_from_slice(&sample.to_le_bytes());
        }
        buf
    })
}

/// Play a notification sound.
/// `preview = true` interrupts any currently playing preview.
/// `preview = false` uses `SND_ASYNC | SND_NODEFAULT | SND_NOSTOP` and never fails chat.
pub fn play_sound(key: &str, preview: bool) -> bool {
    if key.is_empty() {
        return false;
    }
    unsafe {
        if key == BUILTIN_TAP_SOUND {
            let wav = builtin_tap_wav();
            let mut flags = SND_MEMORY | SND_ASYNC | SND_NODEFAULT;
            if !preview {
                flags |= SND_NOSTOP;
            }
            return PlaySoundW(PCWSTR(wav.as_ptr() as *const u16), None, flags).as_bool();
        }

        let root = media_root();
        let candidate = root.join(key);
        let Ok(canonical_root) = root.canonicalize() else {
            return false;
        };
        let Ok(canonical_file) = candidate.canonicalize() else {
            return false;
        };
        if !canonical_file.starts_with(&canonical_root)
            || !is_wav(&canonical_file)
            || !canonical_file.is_file()
        {
            return false;
        }
        let wide = to_wide_null(&canonical_file.to_string_lossy());
        let mut flags = SND_FILENAME | SND_ASYNC | SND_NODEFAULT;
        if !preview {
            flags |= SND_NOSTOP;
        }
        PlaySoundW(PCWSTR(wide.as_ptr()), None, flags).as_bool()
    }
}
