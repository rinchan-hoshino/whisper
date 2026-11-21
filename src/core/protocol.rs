use base64::engine::general_purpose::{STANDARD as B64_STD, URL_SAFE_NO_PAD as B64_URL};
use base64::Engine;
use md5::{Digest as Md5Digest, Md5};
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer as RsaSigner};
use rsa::RsaPrivateKey;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::timeline::ChatMessage;

pub const DEFAULT_BASE_URL: &str = "https://gateway.oopz.cn";
pub const DEFAULT_WS_URL: &str = "wss://ws.oopz.cn";

pub const EVENT_MESSAGE_DELETE: i64 = 8;
pub const EVENT_CHAT_MESSAGE: i64 = 9;
pub const EVENT_AUTH_RESULT: i64 = 21;
pub const EVENT_MESSAGE_EDIT: i64 = 57;
pub const EVENT_SUBSCRIBE_AREA_EVENTS: i64 = 249;
pub const EVENT_AUTH: i64 = 253;
pub const EVENT_HEARTBEAT: i64 = 254;

const BUILTIN_PRIVATE_KEY_PEM: &str = "\
-----BEGIN PRIVATE KEY-----\n\
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC/dbSx0kagPRmh\n\
BdaExF7PPTr4CEc0wNE/pZQXjZwIIiaGruKgw4fPS/steNZLmLo/223cUcW17Vza\n\
/hhvhY9byP4spq9aEGdUjDYHHyBynlUYTdkbcYm2IeCQnQ99uZOPvL/Z6bJIVUXf\n\
Cp2I+8hzslK7aIjNgf15YfHXdi6MPic+6lFCYk38RK+OKXlq4jZxHvM4igVxJcCw\n\
U+dP1D0Y9D/ne4WIijSIz9HTx9ANc3YeR4cxN98loo9zc11cR677midk3Z6Qn0Bs\n\
HeQ48wtz8l1LgYUFiaQKUS7LbtbkfYOHBDDRMi4HQxTZmxh357jaeBNRep4Fij2d\n\
Uqtkc1lRAgMBAAECggEAUQEsAlBFMWaogUhHCsVKl8QAPxDBj8BEx/j2P0Da01Yf\n\
lrxHhl00CLDNBOArSrDDnGDqilzOmbpjnwQCsqx/iwXvBFVBu4lVLqBj+39f6x6y\n\
QiYtxT60VSrgKE9LTpZFqHrjcP8/PvtVJJkJLfvZQ5ofBsujlc95vvGBsPd/mjW2\n\
LPc0+qYGMvSuySQuCmsy5jHA9zLn6TNgAuC6dDhjfD8ovNkPUt7BQn6LyvBj+Zu3\n\
mYzVSDO6aZcyzPe4hakwnFHQ/BDpgLuv+JKKHhdKbJcsgX+00oVGDThqivKqY7Mi\n\
yQIOEvO/AZIVTVIyXwYfsYZUg4noghpOoq2AEJi/QQKBgQDpQu59XGfN3cbW6n/l\n\
OTtkXLL8JSUapjY+5esir150JMxEDEnMuAwpdIxVT0OJdtjHCgMO//3vR57Af3PB\n\
CqjO9QDXhrXvwh4yDmaI8RYRZQibc1HbVsFFE5xP2c2bZcVjU9QIowt03gYSUakm\n\
xNtCM0vutKymejDvgp57PQGAJwKBgQDSH5vKkydAhCuyjCW+5e7u31jX0vPFdI/+\n\
jpNWFgUAv9aw/6omUCQLEU+dhP6/CT0Ej5LFsv1Y6+LAlfNyY+wTO+dVWl7hBDn+\n\
9rtR9+7ri7js5htd4aczzmSWO/WIGnCRr2BlcCpl8fsQIHirE+AC6sXdUey6uGCv\n\
L0XQvntNxwKBgD63nVIdh1eF2tGsSWkDg2GPvxwmjJxmtk0hGqC/0ECgKD/fofQm\n\
RDbdPGIvprQOPWj+2yWsVMWCVz/gxOnRiOPkYi6Q7NgjNGU3FACb+dgBd1f8oLS9\n\
1cdR54RcxniS24jCYE9WL11j2PQR+s4OvflFu5PaBlXYotGzKBNwqrefAoGBAICK\n\
zaeTdgPF12WWztCFoTGIeLUvW+mDcUNsvT2AovbeZNxKyzA8/o+2u1nFroH3iNht\n\
bIB9CveG9mSSVye3cQgjMVXJvxkwmLoc/mMapZJ7G1nxOvSnwr2ymV4+t+nXIt48\n\
x1/GymkrK0emVatysSuXrAvEvmlLufnxlWgp3wypAoGAGWO0G5EC/Ub7eMutFTuK\n\
M3OqZe1QfsA0czqSmRm6BxJScsZ1FUxaTfhgt/gukhcI8VSOLB9V45zOz9ZDurh5\n\
OCDhNYaGJWgvkSUQyywAM0S/ZM6xQ4RdUa3ul7iIFitxwZHBPJvIF1zlc5pD+rje\n\
+rlKfKY1WLUGaXs5QRXfEr4=\n\
-----END PRIVATE KEY-----";

static SEQ_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalOopzSession {
    pub device_id: String,
    pub person_uid: String,
    pub jwt_token: String,
    pub app_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaInfo {
    pub area_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextChannel {
    pub area_id: String,
    pub area_name: String,
    pub channel_id: String,
    pub channel_name: String,
}

#[derive(Debug, Clone)]
pub struct OopzSigner {
    signing_key: SigningKey<Sha256>,
}

impl OopzSigner {
    pub fn builtin() -> Result<Self, String> {
        let priv_key = RsaPrivateKey::from_pkcs8_pem(BUILTIN_PRIVATE_KEY_PEM)
            .map_err(|e| format!("failed to parse builtin RSA key: {e}"))?;
        Ok(Self {
            signing_key: SigningKey::<Sha256>::new(priv_key),
        })
    }

    pub fn body_md5(url_path: &str, body_str: &str) -> String {
        let mut hasher = Md5::new();
        hasher.update(url_path.as_bytes());
        hasher.update(body_str.as_bytes());
        crate::core::updater_core::hex_lower(&hasher.finalize())
    }

    pub fn sign_string(&self, data: &str) -> String {
        let sig = self.signing_key.sign(data.as_bytes());
        B64_STD.encode(sig.to_bytes())
    }

    pub fn sign_request(&self, url_path: &str, body_str: &str, timestamp_ms: &str) -> String {
        let md5 = Self::body_md5(url_path, body_str);
        let mut combined = String::with_capacity(md5.len() + timestamp_ms.len());
        combined.push_str(&md5);
        combined.push_str(timestamp_ms);
        self.sign_string(&combined)
    }

    pub fn build_headers(
        &self,
        session: &LocalOopzSession,
        sign_path: &str,
        body_str: &str,
        timestamp_ms: Option<&str>,
    ) -> Vec<(String, String)> {
        let ts = timestamp_ms
            .map(ToString::to_string)
            .unwrap_or_else(now_ms_string);
        let signature = self.sign_request(sign_path, body_str, &ts);
        let req_id = generate_request_id();
        vec![
            ("Accept".into(), "*/*".into()),
            ("Accept-Language".into(), "zh-CN,zh;q=0.9".into()),
            ("Cache-Control".into(), "no-cache".into()),
            (
                "Content-Type".into(),
                "application/json;charset=utf-8".into(),
            ),
            ("Origin".into(), "https://web.oopz.cn".into()),
            ("Pragma".into(), "no-cache".into()),
            (
                "User-Agent".into(),
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36".into(),
            ),
            ("Oopz-Sign".into(), signature),
            ("Oopz-Request-Id".into(), req_id),
            ("Oopz-Time".into(), ts),
            (
                "Oopz-App-Version-Number".into(),
                if session.app_version.is_empty() {
                    "69514".into()
                } else {
                    session.app_version.clone()
                },
            ),
            ("Oopz-Channel".into(), "Web".into()),
            ("Oopz-Device-Id".into(), session.device_id.clone()),
            ("Oopz-Platform".into(), "windows".into()),
            ("Oopz-Web".into(), "true".into()),
            ("Oopz-Person".into(), session.person_uid.clone()),
            ("Oopz-Signature".into(), session.jwt_token.clone()),
        ]
    }
}

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn now_ms_string() -> String {
    now_ms().to_string()
}

pub fn now_us_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .to_string()
}

pub fn generate_request_id() -> String {
    let n = SEQ_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("w-{}-{n:06x}", now_ms())
}

pub fn generate_client_message_id() -> String {
    let n = SEQ_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}{:04}", now_ms(), n % 10000)
}

/// Parse the base64-encoded JSON string stored in `HKCU\Software\Oopz\OopzData` -> `login`.
pub fn parse_oopz_login_value(raw_b64: &str, now_unix_secs: i64) -> Result<LocalOopzSession, String> {
    let decoded = B64_STD
        .decode(raw_b64.trim())
        .map_err(|_| "本机 Oopz 登录态无法识别，请先更新并重新登录 Oopz。".to_string())?;
    let payload: Value = serde_json::from_slice(&decoded)
        .map_err(|_| "本机 Oopz 登录态格式不完整。".to_string())?;
    let obj = payload
        .as_object()
        .ok_or_else(|| "本机 Oopz 登录态格式不完整。".to_string())?;

    let person_uid = obj
        .get("uid")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let jwt_token = obj
        .get("signature")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let device_id = obj
        .get("deviceId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let app_version = obj
        .get("appVersion")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("69514")
        .to_string();

    if person_uid.is_empty() || jwt_token.is_empty() || device_id.is_empty() {
        return Err("没有找到完整的 Oopz 登录态，请先打开并登录 Oopz。".to_string());
    }

    if let Some(exp) = decode_jwt_exp(&jwt_token) {
        if exp <= now_unix_secs + 60 {
            return Err("Oopz 登录态已经过期，请先打开 Oopz 完成登录或刷新。".to_string());
        }
    }

    Ok(LocalOopzSession {
        device_id,
        person_uid,
        jwt_token,
        app_version,
    })
}

fn decode_jwt_exp(token: &str) -> Option<i64> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let _sig = parts.next()?;
    let decoded = B64_URL.decode(payload.trim_end_matches('=')).ok()?;
    let val: Value = serde_json::from_slice(&decoded).ok()?;
    val.get("exp").and_then(|v| v.as_i64())
}

// --- WebSocket frame builders ---

pub fn build_ws_auth_frame(session: &LocalOopzSession, timestamp_ms: &str) -> String {
    let body = json!({
        "person": session.person_uid,
        "deviceId": session.device_id,
        "signature": session.jwt_token,
        "deviceName": session.device_id,
        "platformName": "web",
        "reconnect": 0
    });
    json!({
        "time": timestamp_ms,
        "body": body.to_string(),
        "event": EVENT_AUTH
    })
    .to_string()
}

pub fn build_ws_subscribe_area_frame(
    areas: &[String],
    uid: &str,
    timestamp_ms: &str,
) -> String {
    let body = json!({
        "areas": areas,
        "type": 1,
        "uid": uid
    });
    json!({
        "time": timestamp_ms,
        "body": body.to_string(),
        "event": EVENT_SUBSCRIBE_AREA_EVENTS
    })
    .to_string()
}

pub fn build_ws_heartbeat_frame(uid: &str, timestamp_ms: &str) -> String {
    let body = json!({
        "person": uid
    });
    json!({
        "time": timestamp_ms,
        "body": body.to_string(),
        "event": EVENT_HEARTBEAT
    })
    .to_string()
}

// --- REST payload builders & parsers ---

pub fn build_send_message_v2_body(area_id: &str, channel_id: &str, text: &str) -> String {
    json!({
        "message": {
            "area": area_id,
            "channel": channel_id,
            "target": "",
            "text": text,
            "content": text,
            "clientMessageId": generate_client_message_id(),
            "timestamp": now_us_string(),
            "isMentionAll": false,
            "mentionList": [],
            "styleTags": [],
            "referenceMessageId": Value::Null,
            "animated": false,
            "displayName": "",
            "duration": 0,
            "attachments": []
        }
    })
    .to_string()
}

pub fn unwrap_api_data(body_bytes: &[u8]) -> Result<Value, String> {
    let root: Value =
        serde_json::from_slice(body_bytes).map_err(|e| format!("响应不是有效 JSON: {e}"))?;
    let obj = root
        .as_object()
        .ok_or_else(|| "响应格式错误：需要 JSON 对象".to_string())?;
    let ok = match obj.get("status") {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        Some(Value::String(s)) => matches!(s.to_ascii_lowercase().as_str(), "true" | "1"),
        _ => false,
    };
    if !ok {
        let msg = obj
            .get("error")
            .or_else(|| obj.get("message"))
            .or_else(|| obj.get("msg"))
            .and_then(|v| v.as_str())
            .unwrap_or("Oopz API 请求失败");
        return Err(msg.to_string());
    }
    obj.get("data")
        .cloned()
        .ok_or_else(|| "响应缺少 data 字段".to_string())
}

pub fn parse_joined_areas(data: &Value) -> Vec<AreaInfo> {
    let mut out = Vec::new();
    let Some(arr) = data.as_array() else {
        return out;
    };
    for item in arr {
        let id = item
            .get("id")
            .or_else(|| item.get("area"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if !id.is_empty() && !name.is_empty() {
            out.push(AreaInfo {
                area_id: id.to_string(),
                name: name.to_string(),
            });
        }
    }
    out
}

pub fn parse_area_text_channels(area_id: &str, area_name: &str, data: &Value) -> Vec<TextChannel> {
    let mut out = Vec::new();
    let Some(groups) = data.as_array() else {
        return out;
    };
    for group in groups {
        let Some(channels) = group.get("channels").and_then(|v| v.as_array()) else {
            continue;
        };
        for ch in channels {
            let ch_type = ch
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_uppercase();
            if ch_type != "TEXT" {
                continue;
            }
            let ch_id = ch
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let ch_name = ch
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if !ch_id.is_empty() && !ch_name.is_empty() {
                out.push(TextChannel {
                    area_id: area_id.to_string(),
                    area_name: area_name.to_string(),
                    channel_id: ch_id.to_string(),
                    channel_name: ch_name.to_string(),
                });
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct RawMessagePayload {
    pub message_id: String,
    pub area_id: String,
    pub channel_id: String,
    pub sender_id: String,
    pub timestamp_us: i64,
    pub text: String,
}

impl RawMessagePayload {
    pub fn into_chat_message(
        self,
        names: &HashMap<String, String>,
        own_uid: &str,
    ) -> ChatMessage {
        let mine = self.sender_id == own_uid;
        let sender_name = names
            .get(&self.sender_id)
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                if mine {
                    "我".to_string()
                } else {
                    let chars: Vec<char> = self.sender_id.chars().collect();
                    if chars.len() > 6 {
                        chars[chars.len() - 6..].iter().collect()
                    } else if !chars.is_empty() {
                        self.sender_id.clone()
                    } else {
                        "未知".to_string()
                    }
                }
            });
        ChatMessage {
            message_id: self.message_id,
            timestamp_us: self.timestamp_us,
            sender_id: self.sender_id,
            sender_name,
            text: self.text,
            mine,
        }
    }
}

pub fn parse_message_object(obj: &Value) -> Option<RawMessagePayload> {
    let map = obj.as_object()?;
    let message_id = value_to_string(map.get("messageId"));
    if message_id.is_empty() {
        return None;
    }
    let area_id = value_to_string(map.get("area"));
    let channel_id = value_to_string(map.get("channel"));
    let sender_id = value_to_string(map.get("person"));
    let ts_str = value_to_string(map.get("timestamp"));
    let timestamp_us = ts_str.parse::<i64>().unwrap_or(0);
    let text = extract_display_text(map);
    Some(RawMessagePayload {
        message_id,
        area_id,
        channel_id,
        sender_id,
        timestamp_us,
        text,
    })
}

fn value_to_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn clean_bidi(s: &str) -> String {
    s.chars()
        .filter(|&c| {
            !matches!(
                c,
                '\u{200e}' | '\u{200f}' | '\u{2066}' | '\u{2067}' | '\u{2068}' | '\u{2069}'
            )
        })
        .collect()
}

fn extract_display_text(map: &serde_json::Map<String, Value>) -> String {
    for key in ["text", "content"] {
        if let Some(s) = map.get(key).and_then(|v| v.as_str()) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                if trimmed.starts_with("![IMAGE") {
                    return "[图片]".to_string();
                }
                return clean_bidi(trimmed);
            }
        }
    }
    if let Some(arr) = map.get("attachments").and_then(|v| v.as_array()) {
        for att in arr {
            let t = att
                .get("attachmentType")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_uppercase();
            if t == "IMAGE" {
                return "[图片]".to_string();
            }
            if t == "VIDEO" {
                return "[视频]".to_string();
            }
            if t == "AUDIO" {
                return "[语音]".to_string();
            }
            if !t.is_empty() {
                return "[文件]".to_string();
            }
        }
    }
    let msg_type = map
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    match msg_type.as_str() {
        "STICKER" => "[表情]".to_string(),
        "IMAGE" => "[图片]".to_string(),
        _ => "[非文字消息]".to_string(),
    }
}

#[derive(Debug, Clone)]
pub enum WsEvent {
    AuthResult { ok: bool },
    ChatMessage(RawMessagePayload),
    MessageEdit(RawMessagePayload),
    MessageDelete {
        area_id: String,
        channel_id: String,
        message_id: String,
    },
    Other(i64),
}

pub fn parse_ws_frame(raw: &str) -> Result<WsEvent, String> {
    let envelope: Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid WS frame JSON: {e}"))?;
    let event_type = envelope
        .get("event")
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or(-1);

    let body: Value = match envelope.get("body") {
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
        Some(v) => v.clone(),
        None => Value::Null,
    };

    match event_type {
        EVENT_AUTH_RESULT => {
            let ok = body
                .get("checkRes")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Ok(WsEvent::AuthResult { ok })
        }
        EVENT_CHAT_MESSAGE | EVENT_MESSAGE_EDIT => {
            let data = match body.get("data") {
                Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
                Some(v) => v.clone(),
                None => Value::Null,
            };
            let msg = parse_message_object(&data)
                .ok_or_else(|| "invalid chat message event data".to_string())?;
            if event_type == EVENT_MESSAGE_EDIT {
                Ok(WsEvent::MessageEdit(msg))
            } else {
                Ok(WsEvent::ChatMessage(msg))
            }
        }
        EVENT_MESSAGE_DELETE => {
            let area_id = value_to_string(body.get("area"));
            let channel_id = value_to_string(body.get("channel"));
            let message_id = value_to_string(body.get("messageId"));
            Ok(WsEvent::MessageDelete {
                area_id,
                channel_id,
                message_id,
            })
        }
        other => Ok(WsEvent::Other(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gate 1: Byte-for-byte parity with Python `oopz_sdk.auth.signer.Signer`.
    #[test]
    fn golden_oopz_sign_matches_python_sdk_byte_for_byte() {
        let signer = OopzSigner::builtin().expect("builtin RSA key loads");

        let vectors = [
            (
                "/userSubscribeArea/v1/list",
                "",
                "1700000000000",
                "c6e1957b6d94663875dbc402fc05498a",
                "D4mqbczpkb08iGOicDAFML5H2J98BXYMcKv9sXy7Bv6fa+YT+awezhXIWOHIIAne5utjMPag+wOurYCGOL8Z9xej/RPbxHXoCJwWOfLmTGLaSuDkJePPcQ7U38NupBaW0kHwrxTZll/Tjf1INvNavZhBCPxCohK3qVt2/ISzZDSB4jWD1eLzpACXhpMrMtbhq7LlEp9l7pZFyXGoZJoxR8Ykt6dkRDGAZaKt/m8YLkbipgPDh7AYhsm0ks4tw3KfRYhiAENhc/knQNMoC34dn1YFBjx812jkgi+8KLWcLMEDcbNY+QE9HCoiKUP0UQDcB2bfhg4M5JgNHY4Jawm95w==",
            ),
            (
                "/client/v1/area/v1/detail/v1/channels?area=area-001",
                "",
                "1700000000123",
                "8ff4a8c2edd1d386cdb0189eb13a2f5d",
                "mNtP/4jdqj+4HQJ6wKTkOGc3CjCpjiPwBmQLmt9yQFNzidIgk7j3Ina8RyqMtevwcfmFjSiI7in3HzTGkM0NjT86PScs6VxcDyAPw3pzkC4baLxAAchWLPqfCRhEb2p9V4MYOQb0GsI4Jup/iSIlRpwY+6uA/6nkRS18LzooFZ2lBPCXSm6Us5iaIE5ws7KILhbWCEjVdb9LrGbxkj2iX9J8fXsN/St9LAc23iA6AaSJJ+uUmLeJDyu7EKM+q6VsvyUAVDoW3UVRG4RdKc5QKkw3HB3HHtDMhfpgI3hU1kPC86DCgSOo9LdT5iwOWqy7aMpAC3f7VIWpQI6hwrZRAA==",
            ),
            (
                "/im/session/v2/sendGimMessage",
                "{\"message\":{\"area\":\"area-001\",\"channel\":\"ch-001\",\"text\":\"你好 Whisper\"}}",
                "1700000000999",
                "442a33087644f0575fa5545f306a4118",
                "hXfNyeN60uaWnidsfFqxWjPoaChkQcPsGdlvbQQgi6BWwPzNd9YCu/ZmATgRUUKDNQw5UCpuiI4re9vsSHoOpWL4Bm6bFbCyotIibicf6NqJtd/7AWM/zmyGXMSN80ldUbO2MOhmTtf+B1XTHMkLzM1aZLuErq1HJnmVdEp45BVUYdWPJK01NeFqC8fp6p6NbAw003Jm7N/R4CTIqdWRDJMZINmPxvCekd5LvHz3SUOwcCjPoOVCaGZa4Nlc7gEuy+p5AtxBOr/yBmlS3XIbPY5HUmoce289w6z0ChUuLuNiwIJgYMDpkWQKqMTbMtChuAVlD7vAiZEgjGzwnUvBXA==",
            ),
        ];

        for (path, body, ts, expected_md5, expected_sig) in vectors {
            assert_eq!(OopzSigner::body_md5(path, body), expected_md5);
            assert_eq!(signer.sign_request(path, body, ts), expected_sig);
        }
    }

    #[test]
    fn ws_frame_builders_and_parsers_roundtrip() {
        let session = LocalOopzSession {
            device_id: "dev-1".into(),
            person_uid: "u-1".into(),
            jwt_token: "tok-1".into(),
            app_version: "69514".into(),
        };
        let auth = build_ws_auth_frame(&session, "1700000000000");
        let v: Value = serde_json::from_str(&auth).unwrap();
        assert_eq!(v["event"], EVENT_AUTH);

        let sub = build_ws_subscribe_area_frame(&["area-1".into()], "u-1", "1700000000001");
        let v2: Value = serde_json::from_str(&sub).unwrap();
        assert_eq!(v2["event"], EVENT_SUBSCRIBE_AREA_EVENTS);

        let chat_frame = json!({
            "event": EVENT_CHAT_MESSAGE,
            "body": json!({
                "data": {
                    "messageId": "m-42",
                    "area": "area-1",
                    "channel": "ch-1",
                    "person": "u-2",
                    "timestamp": "1700000000123456",
                    "text": "A大封烟 QILZU3"
                }
            }).to_string()
        })
        .to_string();

        match parse_ws_frame(&chat_frame).unwrap() {
            WsEvent::ChatMessage(m) => {
                assert_eq!(m.message_id, "m-42");
                assert_eq!(m.area_id, "area-1");
                assert_eq!(m.text, "A大封烟 QILZU3");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let auth_reject = json!({
            "event": EVENT_AUTH_RESULT,
            "body": "{\"checkRes\":false}"
        })
        .to_string();
        match parse_ws_frame(&auth_reject).unwrap() {
            WsEvent::AuthResult { ok } => assert!(!ok),
            other => panic!("unexpected: {other:?}"),
        }

        let del = json!({
            "event": EVENT_MESSAGE_DELETE,
            "body": "{\"area\":\"area-1\",\"channel\":\"ch-1\",\"messageId\":\"m-42\"}"
        })
        .to_string();
        match parse_ws_frame(&del).unwrap() {
            WsEvent::MessageDelete { message_id, .. } => assert_eq!(message_id, "m-42"),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
