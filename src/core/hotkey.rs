/// Cross-platform Win32 virtual-key hotkey specification parser & formatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HotkeySpec {
    pub vk: u32,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl HotkeySpec {
    pub fn modifier_flags(&self) -> u32 {
        const MOD_ALT: u32 = 0x0001;
        const MOD_CONTROL: u32 = 0x0002;
        const MOD_SHIFT: u32 = 0x0004;
        const MOD_WIN: u32 = 0x0008;
        const MOD_NOREPEAT: u32 = 0x4000;

        let mut flags = MOD_NOREPEAT;
        if self.alt {
            flags |= MOD_ALT;
        }
        if self.ctrl {
            flags |= MOD_CONTROL;
        }
        if self.shift {
            flags |= MOD_SHIFT;
        }
        if self.win {
            flags |= MOD_WIN;
        }
        flags
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        let parts: Vec<String> = input
            .split('+')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            return Err("快捷键不能为空".to_string());
        }

        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut win = false;
        let mut main_keys = Vec::new();

        for part in parts {
            match part.as_str() {
                "ctrl" | "control" => ctrl = true,
                "alt" => alt = true,
                "shift" => shift = true,
                "win" | "meta" => win = true,
                _ => main_keys.push(part),
            }
        }

        if main_keys.len() != 1 {
            return Err("快捷键需要且只能包含一个主按键".to_string());
        }

        let name = &main_keys[0];
        let vk = match name.as_str() {
            "enter" | "return" => 0x0D,
            "tab" => 0x09,
            "space" => 0x20,
            "escape" | "esc" => 0x1B,
            "pageup" => 0x21,
            "pagedown" => 0x22,
            "end" => 0x23,
            "home" => 0x24,
            "left" => 0x25,
            "up" => 0x26,
            "right" => 0x27,
            "down" => 0x28,
            "insert" => 0x2D,
            "delete" => 0x2E,
            "`" | "~" | "oem3" | "grave" => 0xC0,
            _ if name.len() == 1 && name.as_bytes()[0].is_ascii_alphanumeric() => {
                name.as_bytes()[0].to_ascii_uppercase() as u32
            }
            _ if name.starts_with('f') => {
                let num: u32 = name[1..]
                    .parse()
                    .map_err(|_| "支持常用功能键、A-Z、0-9 与 F1-F12".to_string())?;
                if (1..=12).contains(&num) {
                    0x70 + num - 1
                } else {
                    return Err("支持常用功能键、A-Z、0-9 与 F1-F12".to_string());
                }
            }
            _ => return Err("支持常用功能键、A-Z、0-9 与 F1-F12".to_string()),
        };

        Ok(Self {
            vk,
            ctrl,
            alt,
            shift,
            win,
        })
    }

    pub fn format(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.win {
            parts.push("Win".to_string());
        }
        let key_name = match self.vk {
            0x0D => "Enter".to_string(),
            0x09 => "Tab".to_string(),
            0x20 => "Space".to_string(),
            0x1B => "Esc".to_string(),
            0x21 => "PageUp".to_string(),
            0x22 => "PageDown".to_string(),
            0x23 => "End".to_string(),
            0x24 => "Home".to_string(),
            0x25 => "Left".to_string(),
            0x26 => "Up".to_string(),
            0x27 => "Right".to_string(),
            0x28 => "Down".to_string(),
            0x2D => "Insert".to_string(),
            0x2E => "Delete".to_string(),
            0xC0 => "`".to_string(),
            0x70..=0x7B => format!("F{}", self.vk - 0x70 + 1),
            0x30..=0x39 | 0x41..=0x5A => (self.vk as u8 as char).to_string(),
            other => format!("0x{other:X}"),
        };
        parts.push(key_name);
        parts.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_hotkeys() {
        let f8 = HotkeySpec::parse("F8").unwrap();
        assert_eq!(f8.vk, 0x77);
        assert_eq!(f8.format(), "F8");

        let combo = HotkeySpec::parse("ctrl + shift + k").unwrap();
        assert!(combo.ctrl && combo.shift && !combo.alt);
        assert_eq!(combo.vk, b'K' as u32);
        assert_eq!(combo.format(), "Ctrl+Shift+K");
    }
}
