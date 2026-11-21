use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DEFAULT_MANIFEST_URL: &str = "https://acv.k-neco.com/tools/whisper/latest.json";
pub const ALLOWED_ORIGIN_PREFIX: &str = "https://acv.k-neco.com/";

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
}
