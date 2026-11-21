use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DEFAULT_MANIFEST_URL: &str = "https://acv.k-neco.com/tools/whisper/latest.json";
pub const ALLOWED_ORIGIN_PREFIX: &str = "https://acv.k-neco.com/";
pub const DEFAULT_AUTO_UPDATE_INTERVAL_MS: u32 = 1_800_000;
pub const INITIAL_AUTO_UPDATE_DELAY_MS: u32 = 5_000;
pub const MIN_AUTO_UPDATE_INTERVAL_MS: u32 = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSilentUpdate {
    pub version: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UpdateIdleState {
    pub hud_active: bool,
    pub adjust_mode: bool,
    pub settings_visible: bool,
    pub has_draft: bool,
}

impl UpdateIdleState {
    pub fn is_idle(self) -> bool {
        !self.hud_active && !self.adjust_mode && !self.settings_visible && !self.has_draft
    }
}

/// Coordinates background silent update checks and defers atomic self-replacement
/// until the UI returns to the passive idle HUD state.
#[derive(Debug, Default)]
pub struct SilentUpdateCoordinator {
    in_flight: bool,
    pending: Option<PendingSilentUpdate>,
}

impl SilentUpdateCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn should_start_check(&self) -> bool {
        !self.in_flight && self.pending.is_none()
    }

    pub fn mark_check_started(&mut self) {
        self.in_flight = true;
    }

    pub fn on_check_finished_no_update(&mut self) {
        self.in_flight = false;
    }

    pub fn on_check_finished_ready(&mut self, version: String, bytes: Vec<u8>) {
        self.in_flight = false;
        self.pending = Some(PendingSilentUpdate { version, bytes });
    }

    pub fn take_if_idle(&mut self, idle: UpdateIdleState) -> Option<PendingSilentUpdate> {
        if idle.is_idle() {
            self.pending.take()
        } else {
            None
        }
    }

    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn is_in_flight(&self) -> bool {
        self.in_flight
    }
}

pub fn auto_update_interval_ms(env_val: Option<&str>) -> u32 {
    env_val
        .and_then(|v| v.trim().parse::<u32>().ok())
        .map(|ms| ms.max(MIN_AUTO_UPDATE_INTERVAL_MS))
        .unwrap_or(DEFAULT_AUTO_UPDATE_INTERVAL_MS)
}

pub fn initial_auto_update_delay_ms(interval_ms: u32) -> u32 {
    interval_ms.min(INITIAL_AUTO_UPDATE_DELAY_MS)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

pub fn version_key(version: &str) -> Result<Vec<u32>, String> {
    let trimmed = version.trim().trim_start_matches('v');
    if trimmed.is_empty() {
        return Err("empty version".to_string());
    }
    trimmed
        .split('.')
        .map(|part| {
            part.parse::<u32>()
                .map_err(|_| format!("invalid version component: {part}"))
        })
        .collect()
}

pub fn is_newer_version(remote: &str, current: &str) -> Result<bool, String> {
    Ok(version_key(remote)? > version_key(current)?)
}

pub fn parse_manifest(payload: &[u8], allow_loopback: bool) -> Result<UpdateManifest, String> {
    let mut manifest: UpdateManifest =
        serde_json::from_slice(payload).map_err(|e| format!("invalid manifest JSON: {e}"))?;
    manifest.sha256 = manifest.sha256.trim().to_ascii_lowercase();
    version_key(&manifest.version)?;

    let valid_origin = manifest.url.starts_with(ALLOWED_ORIGIN_PREFIX)
        || (allow_loopback
            && (manifest.url.starts_with("http://127.0.0.1:")
                || manifest.url.starts_with("http://localhost:")));
    if !valid_origin {
        return Err("update URL must use the ACV HTTPS origin".to_string());
    }
    if manifest.sha256.len() != 64 || !manifest.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid update SHA-256".to_string());
    }
    if manifest.size == 0 {
        return Err("invalid update size".to_string());
    }
    Ok(manifest)
}

pub fn verify_bytes(bytes: &[u8], expected_sha256: &str, expected_size: u64) -> Result<(), String> {
    if bytes.len() as u64 != expected_size {
        return Err(format!(
            "size mismatch: got {} bytes, expected {} bytes",
            bytes.len(),
            expected_size
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual = hex_lower(&hasher.finalize());
    if actual != expected_sha256.to_ascii_lowercase() {
        return Err(format!(
            "SHA-256 mismatch: got {actual}, expected {expected_sha256}"
        ));
    }
    Ok(())
}

pub fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Returns true if a binary or shortcut stem (without `.exe`/`.lnk`) belongs to a legacy pre-Whisper release.
pub fn is_legacy_app_stem(stem: &str) -> bool {
    let trimmed = stem.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower == "whisper" {
        return false;
    }
    lower.starts_with("oopz")
        || lower.starts_with("typeslip")
        || lower.starts_with("papernote")
        || trimmed.contains("纸条")
        || trimmed.contains("战术浮层")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_stem_detection() {
        assert!(is_legacy_app_stem("OopzOverlay"));
        assert!(is_legacy_app_stem("OopzTacticalOverlay"));
        assert!(is_legacy_app_stem("TypeSlip"));
        assert!(is_legacy_app_stem("PaperNote"));
        assert!(is_legacy_app_stem("纸条"));
        assert!(is_legacy_app_stem("Oopz 战术浮层"));
        assert!(!is_legacy_app_stem("Whisper"));
        assert!(!is_legacy_app_stem("whisper"));
        assert!(!is_legacy_app_stem("Notepad"));
    }

    #[test]
    fn manifest_origin_and_sha256_validation() {
        let body = b"fake whisper binary 2.0.1";
        let mut h = Sha256::new();
        h.update(body);
        let sha = hex_lower(&h.finalize());

        let valid_json = format!(
            r#"{{"version":"2.0.1","url":"https://acv.k-neco.com/tools/whisper/Whisper-2.0.1.exe","sha256":"{sha}","size":{}}}"#,
            body.len()
        );
        let m = parse_manifest(valid_json.as_bytes(), false).unwrap();
        assert!(is_newer_version(&m.version, "2.0.0").unwrap());
        assert!(!is_newer_version("2.0.0", "2.0.0").unwrap());
        verify_bytes(body, &m.sha256, m.size).unwrap();

        // Tamper 1 byte -> rejected!
        let mut tampered = body.to_vec();
        tampered[0] ^= 0x01;
        assert!(verify_bytes(&tampered, &m.sha256, m.size).is_err());

        // Disallowed origin -> rejected!
        let bad_origin = format!(
            r#"{{"version":"2.0.1","url":"https://evil.example.com/Whisper.exe","sha256":"{sha}","size":{}}}"#,
            body.len()
        );
        assert!(parse_manifest(bad_origin.as_bytes(), false).is_err());
    }

    #[test]
    fn silent_update_defers_while_busy_and_applies_on_idle() {
        let mut coord = SilentUpdateCoordinator::new();
        assert!(coord.should_start_check());
        coord.mark_check_started();
        assert!(!coord.should_start_check());
        assert!(coord.is_in_flight());

        let payload = b"new-whisper-2.0.3".to_vec();
        coord.on_check_finished_ready("2.0.3".to_string(), payload.clone());
        assert!(!coord.is_in_flight());
        assert!(coord.has_pending());
        // Already staged -> do not start redundant background checks
        assert!(!coord.should_start_check());

        // Busy in hud_active -> deferred
        assert!(coord
            .take_if_idle(UpdateIdleState {
                hud_active: true,
                adjust_mode: false,
                settings_visible: false,
                has_draft: false,
            })
            .is_none());
        assert!(coord.has_pending());

        // Busy in adjust_mode -> deferred
        assert!(coord
            .take_if_idle(UpdateIdleState {
                hud_active: false,
                adjust_mode: true,
                settings_visible: false,
                has_draft: false,
            })
            .is_none());

        // Busy in settings_visible -> deferred
        assert!(coord
            .take_if_idle(UpdateIdleState {
                hud_active: false,
                adjust_mode: false,
                settings_visible: true,
                has_draft: false,
            })
            .is_none());

        // Busy with non-empty draft -> deferred
        assert!(coord
            .take_if_idle(UpdateIdleState {
                hud_active: false,
                adjust_mode: false,
                settings_visible: false,
                has_draft: true,
            })
            .is_none());

        // Exits hud_active / all idle -> immediately consumed and applied
        let ready = coord
            .take_if_idle(UpdateIdleState::default())
            .expect("should apply once idle");
        assert_eq!(ready.version, "2.0.3");
        assert_eq!(ready.bytes, payload);
        assert!(!coord.has_pending());
        assert!(coord.take_if_idle(UpdateIdleState::default()).is_none());
    }

    #[test]
    fn silent_update_no_update_clears_in_flight() {
        let mut coord = SilentUpdateCoordinator::new();
        coord.mark_check_started();
        coord.on_check_finished_no_update();
        assert!(!coord.is_in_flight());
        assert!(!coord.has_pending());
        assert!(coord.should_start_check());
    }

    #[test]
    fn auto_update_interval_parsing_and_clamping() {
        assert_eq!(auto_update_interval_ms(None), 1_800_000);
        assert_eq!(auto_update_interval_ms(Some("invalid")), 1_800_000);
        assert_eq!(auto_update_interval_ms(Some("2000")), 2_000);
        assert_eq!(auto_update_interval_ms(Some("100")), 500);

        assert_eq!(initial_auto_update_delay_ms(1_800_000), 5_000);
        assert_eq!(initial_auto_update_delay_ms(2_000), 2_000);
    }
}
