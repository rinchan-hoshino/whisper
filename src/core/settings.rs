use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILTIN_TAP_SOUND: &str = "__builtin_tap__";
pub const DEFAULT_NOTIFICATION_SOUND: &str = "Windows Navigation Start.wav";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Default for DockCorner {
    fn default() -> Self {
        Self::BottomLeft
    }
}

impl DockCorner {
    pub fn from_position(position_x: f64, position_y: f64) -> Self {
        match (position_x < 0.5, position_y < 0.5) {
            (true, true) => Self::TopLeft,
            (false, true) => Self::TopRight,
            (true, false) => Self::BottomLeft,
            (false, false) => Self::BottomRight,
        }
    }

    pub fn default_position(self) -> (f64, f64) {
        match self {
            Self::TopLeft => (0.16, 0.16),
            Self::TopRight => (0.84, 0.16),
            Self::BottomLeft => (0.16, 0.82),
            Self::BottomRight => (0.84, 0.82),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub area_id: String,
    #[serde(default)]
    pub area_name: String,
    #[serde(default)]
    pub channel_id: String,
    #[serde(default)]
    pub channel_name: String,
    #[serde(default)]
    pub screen_name: String,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_visibility_hotkey")]
    pub visibility_hotkey: String,
    #[serde(default = "default_position_x")]
    pub position_x: f64,
    #[serde(default = "default_position_y")]
    pub position_y: f64,
    #[serde(default)]
    pub overlay_width: i32,
    #[serde(default)]
    pub overlay_height: i32,
    #[serde(default = "default_always_visible")]
    pub always_visible: bool,
    #[serde(default = "default_font_size")]
    pub font_size: i32,
    #[serde(default = "default_text_opacity")]
    pub text_opacity: i32,
    #[serde(default = "default_backdrop_opacity")]
    pub backdrop_opacity: i32,
    #[serde(default = "default_notification_enabled")]
    pub notification_enabled: bool,
    #[serde(default = "default_notification_sound")]
    pub notification_sound: String,
    #[serde(default)]
    pub launch_on_startup: bool,
    #[serde(default)]
    pub dock_corner: Option<DockCorner>,

    // Non-persisted runtime credentials
    #[serde(skip)]
    pub device_id: String,
    #[serde(skip)]
    pub person_uid: String,
    #[serde(skip)]
    pub jwt_token: String,
    #[serde(skip)]
    pub app_version: String,
}

fn default_schema_version() -> u32 {
    20
}
fn default_hotkey() -> String {
    "F8".to_string()
}
fn default_visibility_hotkey() -> String {
    "F9".to_string()
}
fn default_position_x() -> f64 {
    0.16
}
fn default_position_y() -> f64 {
    0.82
}
fn default_always_visible() -> bool {
    false
}
fn default_font_size() -> i32 {
    15
}
fn default_text_opacity() -> i32 {
    95
}
fn default_backdrop_opacity() -> i32 {
    18
}
fn default_notification_enabled() -> bool {
    true
}
fn default_notification_sound() -> String {
    DEFAULT_NOTIFICATION_SOUND.to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 20,
            area_id: String::new(),
            area_name: String::new(),
            channel_id: String::new(),
            channel_name: String::new(),
            screen_name: String::new(),
            hotkey: default_hotkey(),
            visibility_hotkey: default_visibility_hotkey(),
            position_x: default_position_x(),
            position_y: default_position_y(),
            overlay_width: 0,
            overlay_height: 0,
            always_visible: default_always_visible(),
            font_size: default_font_size(),
            text_opacity: default_text_opacity(),
            backdrop_opacity: default_backdrop_opacity(),
            notification_enabled: default_notification_enabled(),
            notification_sound: default_notification_sound(),
            launch_on_startup: false,
            dock_corner: Some(DockCorner::BottomLeft),
            device_id: String::new(),
            person_uid: String::new(),
            jwt_token: String::new(),
            app_version: "69514".to_string(),
        }
    }
}

impl AppSettings {
    pub fn effective_dock_corner(&self) -> DockCorner {
        self.dock_corner
            .unwrap_or_else(|| DockCorner::from_position(self.position_x, self.position_y))
    }

    pub fn has_credentials(&self) -> bool {
        !self.device_id.is_empty() && !self.person_uid.is_empty() && !self.jwt_token.is_empty()
    }

    pub fn is_ready(&self) -> bool {
        self.has_credentials() && !self.area_id.is_empty() && !self.channel_id.is_empty()
    }

    /// Parse either 2.0 Whisper JSON or 1.x TypeSlip / OopzTacticalOverlay JSON losslessly.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, String> {
        let raw: Value =
            serde_json::from_slice(bytes).map_err(|e| format!("invalid settings JSON: {e}"))?;
        let obj = raw
            .as_object()
            .ok_or_else(|| "settings payload must be a JSON object".to_string())?;

        let raw_schema = obj
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u32;

        let mut settings = Self::default();
        if let Some(s) = obj.get("area_id").and_then(|v| v.as_str()) {
            settings.area_id = s.to_string();
        }
        if let Some(s) = obj.get("area_name").and_then(|v| v.as_str()) {
            settings.area_name = s.to_string();
        }
        if let Some(s) = obj.get("channel_id").and_then(|v| v.as_str()) {
            settings.channel_id = s.to_string();
        }
        if let Some(s) = obj.get("channel_name").and_then(|v| v.as_str()) {
            settings.channel_name = s.to_string();
        }
        if let Some(s) = obj.get("screen_name").and_then(|v| v.as_str()) {
            settings.screen_name = s.to_string();
        }
        if let Some(s) = obj.get("hotkey").and_then(|v| v.as_str()) {
            if !s.trim().is_empty() {
                settings.hotkey = s.trim().to_string();
            }
        }
        if let Some(s) = obj.get("visibility_hotkey").and_then(|v| v.as_str()) {
            if !s.trim().is_empty() {
                settings.visibility_hotkey = s.trim().to_string();
            }
        }

        // Preserve 1.x legacy schema_version=9 strength delta migration if encountered
        let delta: i32 = if raw_schema == 9 {
            let strength = obj
                .get("strength")
                .and_then(|v| v.as_i64())
                .unwrap_or(50)
                .clamp(0, 100) as i32;
            strength - 50
        } else {
            0
        };

        if let Some(v) = obj.get("position_x").and_then(|v| v.as_f64()) {
            if v.is_finite() {
                settings.position_x = v.clamp(0.0, 1.0);
            }
        }
        if let Some(v) = obj.get("position_y").and_then(|v| v.as_f64()) {
            if v.is_finite() {
                settings.position_y = v.clamp(0.0, 1.0);
            }
        }
        if let Some(v) = obj.get("overlay_width").and_then(|v| v.as_i64()) {
            settings.overlay_width = v.clamp(0, 16000) as i32;
        }
        if let Some(v) = obj.get("overlay_height").and_then(|v| v.as_i64()) {
            settings.overlay_height = v.clamp(0, 16000) as i32;
        }
        if let Some(v) = obj.get("always_visible").and_then(|v| v.as_bool()) {
            settings.always_visible = v;
        }
        if let Some(v) = obj.get("font_size").and_then(|v| v.as_i64()) {
            settings.font_size = (v as i32).clamp(10, 24);
        }
        if let Some(v) = obj.get("text_opacity").and_then(|v| v.as_i64()) {
            let adjusted = v as i32 + ((delta as f64) * 0.8).round() as i32;
            settings.text_opacity = adjusted.clamp(20, 100);
        }
        if let Some(v) = obj.get("backdrop_opacity").and_then(|v| v.as_i64()) {
            settings.backdrop_opacity = (v as i32 + delta).clamp(0, 85);
        }

        // Losslessly preserve 1.x notification_sound string (including empty string meaning muted in 1.x)
        if let Some(sound) = obj.get("notification_sound").and_then(|v| v.as_str()) {
            settings.notification_sound = sound.to_string();
            if raw_schema < 20 {
                // In 1.x, empty notification_sound meant disabled; non-empty meant enabled
                settings.notification_enabled = !sound.is_empty();
                if sound.is_empty() {
                    settings.notification_sound = DEFAULT_NOTIFICATION_SOUND.to_string();
                }
            }
        }
        if let Some(v) = obj.get("notification_enabled").and_then(|v| v.as_bool()) {
            settings.notification_enabled = v;
        }
        if let Some(v) = obj.get("launch_on_startup").and_then(|v| v.as_bool()) {
            settings.launch_on_startup = v;
        }
        if let Some(corner_val) = obj.get("dock_corner") {
            if let Ok(corner) = serde_json::from_value::<DockCorner>(corner_val.clone()) {
                settings.dock_corner = Some(corner);
            }
        } else {
            settings.dock_corner = Some(DockCorner::from_position(
                settings.position_x,
                settings.position_y,
            ));
        }

        settings.schema_version = 20;
        Ok(settings)
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(self).map_err(|e| format!("serialize settings failed: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_1_0_4_settings_losslessly() {
        let legacy_json = br#"{
            "schema_version": 10,
            "area_id": "area-999",
            "area_name": "CatSquad",
            "channel_id": "ch-101",
            "channel_name": "tactical",
            "screen_name": "DISPLAY1",
            "hotkey": "Alt+F8",
            "visibility_hotkey": "F10",
            "position_x": 0.85,
            "position_y": 0.18,
            "overlay_width": 420,
            "overlay_height": 220,
            "always_visible": true,
            "font_size": 14,
            "text_opacity": 92,
            "backdrop_opacity": 25,
            "notification_sound": "Landscape/Windows Navigation Start.wav"
        }"#;

        let migrated = AppSettings::from_json_bytes(legacy_json).unwrap();
        assert_eq!(migrated.schema_version, 20);
        assert_eq!(migrated.area_id, "area-999");
        assert_eq!(migrated.channel_id, "ch-101");
        assert_eq!(migrated.hotkey, "Alt+F8");
        assert_eq!(migrated.visibility_hotkey, "F10");
        assert!((migrated.position_x - 0.85).abs() < 1e-6);
        assert!((migrated.position_y - 0.18).abs() < 1e-6);
        assert_eq!(migrated.overlay_width, 420);
        assert_eq!(migrated.overlay_height, 220);
        assert!(migrated.always_visible);
        assert_eq!(migrated.font_size, 14);
        assert_eq!(migrated.text_opacity, 92);
        assert_eq!(migrated.backdrop_opacity, 25);
        assert!(migrated.notification_enabled);
        assert_eq!(
            migrated.notification_sound,
            "Landscape/Windows Navigation Start.wav"
        );
        assert_eq!(migrated.effective_dock_corner(), DockCorner::TopRight);
    }

    #[test]
    fn legacy_empty_sound_sets_notification_disabled() {
        let legacy_json = br#"{"schema_version":10,"notification_sound":""}"#;
        let migrated = AppSettings::from_json_bytes(legacy_json).unwrap();
        assert!(!migrated.notification_enabled);
    }
}
