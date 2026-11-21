use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::core::lifecycle::SessionProbe;
use crate::core::protocol::{
    build_send_message_v2_body, build_ws_auth_frame, build_ws_heartbeat_frame,
    build_ws_subscribe_area_frame, now_ms_string, parse_area_text_channels, parse_joined_areas,
    parse_message_object, parse_ws_frame, unwrap_api_data, AreaInfo, LocalOopzSession, OopzSigner,
    RawMessagePayload, TextChannel, WsEvent, DEFAULT_BASE_URL, DEFAULT_WS_URL,
};
use crate::core::timeline::ChatMessage;
use crate::platform::winhttp::{http_request, WinHttpWebSocket};

pub const WM_APP_GATEWAY_EVENT: u32 = WM_APP + 102;

#[derive(Debug, Clone)]
pub enum GatewayEvent {
    SessionLoaded(SessionProbe),
    AreasLoaded {
        areas: Vec<AreaInfo>,
    },
    ChannelsLoaded {
        area_id: String,
        channels: Vec<TextChannel>,
    },
    StatusChanged {
        generation: u64,
        connected: bool,
        text: String,
    },
    HistoryLoaded {
        generation: u64,
        messages: Vec<ChatMessage>,
    },
    LiveMessage {
        generation: u64,
        message: ChatMessage,
    },
    MessageDeleted {
        generation: u64,
        message_id: String,
    },
    SendResult {
        ok: bool,
        error: Option<String>,
    },
    AuthRejected(String),
    Error(String),
}

pub struct GatewayHandle {
    notify_hwnd: usize,
    event_tx: mpsc::Sender<GatewayEvent>,
    signer: Arc<OopzSigner>,
    generation: Arc<AtomicU64>,
    ws_stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    active_ws: Arc<Mutex<Option<Arc<WinHttpWebSocket>>>>,
    session_check_in_flight: Arc<AtomicBool>,
    names: Arc<Mutex<HashMap<String, String>>>,
}

impl GatewayHandle {
    pub fn new(notify_hwnd: HWND) -> (Self, mpsc::Receiver<GatewayEvent>) {
        let (tx, rx) = mpsc::channel();
        let signer = Arc::new(OopzSigner::builtin().expect("builtin Oopz signer key"));
        (
            Self {
                notify_hwnd: notify_hwnd.0 as usize,
                event_tx: tx,
                signer,
                generation: Arc::new(AtomicU64::new(0)),
                ws_stop: Arc::new(Mutex::new(None)),
                active_ws: Arc::new(Mutex::new(None)),
                session_check_in_flight: Arc::new(AtomicBool::new(false)),
                names: Arc::new(Mutex::new(HashMap::new())),
            },
            rx,
        )
    }

    pub fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn check_session(&self) {
        if self
            .session_check_in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let hwnd = self.notify_hwnd;
        let tx = self.event_tx.clone();
        let in_flight = Arc::clone(&self.session_check_in_flight);
        thread::spawn(move || {
            let res = crate::platform::session::probe_local_oopz_session();
            in_flight.store(false, Ordering::SeqCst);
            emit_to_ui(hwnd, &tx, GatewayEvent::SessionLoaded(res));
        });
    }

    pub fn fetch_areas(&self, session: LocalOopzSession) {
        let hwnd = self.notify_hwnd;
        let tx = self.event_tx.clone();
        let signer = Arc::clone(&self.signer);
        thread::spawn(move || {
            let mut merged: HashMap<String, AreaInfo> = HashMap::new();
            let mut last_err = None;
            for path in [
                "/userSubscribeArea/v1/list",
                "/client/v1/area/v1/home/v1/subscribed",
            ] {
                match signed_request(&signer, &session, "GET", path, None) {
                    Ok(data) => {
                        for area in parse_joined_areas(&data) {
                            merged.entry(area.area_id.clone()).or_insert(area);
                        }
                    }
                    Err(e) => last_err = Some(e),
                }
            }
            if merged.is_empty() {
                if let Some(err) = last_err {
                    emit_to_ui(hwnd, &tx, GatewayEvent::Error(err));
                    return;
                }
            }
            let mut areas: Vec<AreaInfo> = merged.into_values().collect();
            areas.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            emit_to_ui(hwnd, &tx, GatewayEvent::AreasLoaded { areas });
        });
    }

    pub fn fetch_channels(&self, session: LocalOopzSession, area_id: String, area_name: String) {
        let hwnd = self.notify_hwnd;
        let tx = self.event_tx.clone();
        let signer = Arc::clone(&self.signer);
        thread::spawn(move || {
            let path = format!("/client/v1/area/v1/detail/v1/channels?area={area_id}");
            match signed_request(&signer, &session, "GET", &path, None) {
                Ok(data) => {
                    let channels = parse_area_text_channels(&area_id, &area_name, &data);
                    emit_to_ui(hwnd, &tx, GatewayEvent::ChannelsLoaded { area_id, channels });
                }
                Err(e) => emit_to_ui(hwnd, &tx, GatewayEvent::Error(e)),
            }
        });
    }

    pub fn disconnect(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut guard) = self.ws_stop.lock() {
            if let Some(flag) = guard.take() {
                flag.store(true, Ordering::SeqCst);
            }
        }
        if let Ok(mut guard) = self.active_ws.lock() {
            if let Some(ws) = guard.take() {
                thread::spawn(move || {
                    ws.close();
                });
            }
        }
    }

    pub fn connect_channel(
        &self,
        session: LocalOopzSession,
        area_id: String,
        channel_id: String,
        channel_name: String,
    ) {
        self.disconnect();
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let stop_flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut guard) = self.ws_stop.lock() {
            *guard = Some(Arc::clone(&stop_flag));
        }

        let hwnd = self.notify_hwnd;
        let tx = self.event_tx.clone();
        let signer = Arc::clone(&self.signer);
        let gen_counter = Arc::clone(&self.generation);
        let active_ws = Arc::clone(&self.active_ws);
        let names = Arc::clone(&self.names);

        thread::spawn(move || {
            let mut backoff_secs = 2u64;
            while !stop_flag.load(Ordering::SeqCst) && gen_counter.load(Ordering::SeqCst) == gen {
                let ws_url = ws_base_url();
                let ws_headers = vec![
                    ("Origin".to_string(), "https://web.oopz.cn".to_string()),
                    (
                        "User-Agent".to_string(),
                        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36".to_string(),
                    ),
                ];

                match WinHttpWebSocket::connect(&ws_url, &ws_headers) {
                    Ok(ws) => {
                        backoff_secs = 2;
                        let ws = Arc::new(ws);
                        if stop_flag.load(Ordering::SeqCst)
                            || gen_counter.load(Ordering::SeqCst) != gen
                        {
                            ws.close();
                            break;
                        }
                        if let Ok(mut guard) = active_ws.lock() {
                            *guard = Some(Arc::clone(&ws));
                        }

                        // 1. Send AUTH (253)
                        let auth_frame = build_ws_auth_frame(&session, &now_ms_string());
                        if ws.send_text(&auth_frame).is_err() {
                            ws.close();
                            continue;
                        }

                        // 2. Send SUBSCRIBE_AREA_EVENTS (249)
                        let sub_frame = build_ws_subscribe_area_frame(
                            std::slice::from_ref(&area_id),
                            &session.person_uid,
                            &now_ms_string(),
                        );
                        if ws.send_text(&sub_frame).is_err() {
                            ws.close();
                            continue;
                        }

                        emit_to_ui(
                            hwnd,
                            &tx,
                            GatewayEvent::StatusChanged {
                                generation: gen,
                                connected: true,
                                text: format!("#{channel_name}"),
                            },
                        );

                        // 3. Fetch history in parallel
                        {
                            let h_signer = Arc::clone(&signer);
                            let h_session = session.clone();
                            let h_area = area_id.clone();
                            let h_channel = channel_id.clone();
                            let h_names = Arc::clone(&names);
                            let h_tx = tx.clone();
                            thread::spawn(move || {
                                let path = format!(
                                    "/im/session/v2/messageBefore?area={h_area}&channel={h_channel}&size=80"
                                );
                                if let Ok(data) =
                                    signed_request(&h_signer, &h_session, "GET", &path, None)
                                {
                                    if let Some(arr) =
                                        data.get("messages").and_then(|v| v.as_array())
                                    {
                                        let raws: Vec<RawMessagePayload> =
                                            arr.iter().filter_map(parse_message_object).collect();
                                        resolve_names(&h_signer, &h_session, &raws, &h_names);
                                        let map = h_names.lock().unwrap().clone();
                                        let msgs = raws
                                            .into_iter()
                                            .map(|r| r.into_chat_message(&map, &h_session.person_uid))
                                            .collect();
                                        emit_to_ui(
                                            hwnd,
                                            &h_tx,
                                            GatewayEvent::HistoryLoaded {
                                                generation: gen,
                                                messages: msgs,
                                            },
                                        );
                                    }
                                }
                            });
                        }

                        // 4. Spawn heartbeat thread (every 10s)
                        let hb_done = Arc::new(AtomicBool::new(false));
                        {
                            let hb_ws = Arc::clone(&ws);
                            let hb_stop = Arc::clone(&stop_flag);
                            let hb_done_c = Arc::clone(&hb_done);
                            let hb_uid = session.person_uid.clone();
                            thread::spawn(move || {
                                while !hb_done_c.load(Ordering::SeqCst)
                                    && !hb_stop.load(Ordering::SeqCst)
                                {
                                    for _ in 0..20 {
                                        if hb_done_c.load(Ordering::SeqCst)
                                            || hb_stop.load(Ordering::SeqCst)
                                        {
                                            return;
                                        }
                                        thread::sleep(Duration::from_millis(500));
                                    }
                                    let frame = build_ws_heartbeat_frame(&hb_uid, &now_ms_string());
                                    if hb_ws.send_text(&frame).is_err() {
                                        return;
                                    }
                                }
                            });
                        }

                        // 5. Receive loop
                        let mut auth_rejected = false;
                        while !stop_flag.load(Ordering::SeqCst)
                            && gen_counter.load(Ordering::SeqCst) == gen
                        {
                            match ws.recv_text() {
                                Ok(Some(raw)) => {
                                    if let Ok(ev) = parse_ws_frame(&raw) {
                                        match ev {
                                            WsEvent::AuthResult { ok: false } => {
                                                auth_rejected = true;
                                                emit_to_ui(
                                                    hwnd,
                                                    &tx,
                                                    GatewayEvent::AuthRejected(
                                                        "Oopz 登录态已失效，请重新登录 Oopz。"
                                                            .to_string(),
                                                    ),
                                                );
                                                break;
                                            }
                                            WsEvent::ChatMessage(msg)
                                            | WsEvent::MessageEdit(msg) => {
                                                if msg.area_id == area_id
                                                    && msg.channel_id == channel_id
                                                {
                                                    resolve_names(
                                                        &signer,
                                                        &session,
                                                        std::slice::from_ref(&msg),
                                                        &names,
                                                    );
                                                    let map = names.lock().unwrap().clone();
                                                    let chat = msg.into_chat_message(
                                                        &map,
                                                        &session.person_uid,
                                                    );
                                                    emit_to_ui(
                                                        hwnd,
                                                        &tx,
                                                        GatewayEvent::LiveMessage {
                                                            generation: gen,
                                                            message: chat,
                                                        },
                                                    );
                                                }
                                            }
                                            WsEvent::MessageDelete {
                                                area_id: a,
                                                channel_id: c,
                                                message_id,
                                            } => {
                                                if a == area_id && c == channel_id {
                                                    emit_to_ui(
                                                        hwnd,
                                                        &tx,
                                                        GatewayEvent::MessageDeleted {
                                                            generation: gen,
                                                            message_id,
                                                        },
                                                    );
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                Ok(None) | Err(_) => break,
                            }
                        }

                        hb_done.store(true, Ordering::SeqCst);
                        if let Ok(mut guard) = active_ws.lock() {
                            if guard.as_ref().is_some_and(|cur| Arc::ptr_eq(cur, &ws)) {
                                *guard = None;
                            }
                        }
                        ws.close();
                        if auth_rejected {
                            break;
                        }
                    }
                    Err(_) => {}
                }

                if stop_flag.load(Ordering::SeqCst) || gen_counter.load(Ordering::SeqCst) != gen {
                    break;
                }
                emit_to_ui(
                    hwnd,
                    &tx,
                    GatewayEvent::StatusChanged {
                        generation: gen,
                        connected: false,
                        text: "连接中断，正在重连…".to_string(),
                    },
                );
                for _ in 0..(backoff_secs * 2) {
                    if stop_flag.load(Ordering::SeqCst)
                        || gen_counter.load(Ordering::SeqCst) != gen
                    {
                        return;
                    }
                    thread::sleep(Duration::from_millis(500));
                }
                backoff_secs = (backoff_secs * 2).min(30);
            }
        });
    }

    pub fn send_message(
        &self,
        session: LocalOopzSession,
        area_id: String,
        channel_id: String,
        text: String,
    ) {
        let hwnd = self.notify_hwnd;
        let tx = self.event_tx.clone();
        let signer = Arc::clone(&self.signer);
        thread::spawn(move || {
            let body = build_send_message_v2_body(&area_id, &channel_id, &text);
            match signed_request(
                &signer,
                &session,
                "POST",
                "/im/session/v2/sendGimMessage",
                Some(&body),
            ) {
                Ok(_) => emit_to_ui(hwnd, &tx, GatewayEvent::SendResult { ok: true, error: None }),
                Err(e) => emit_to_ui(
                    hwnd,
                    &tx,
                    GatewayEvent::SendResult {
                        ok: false,
                        error: Some(e),
                    },
                ),
            }
        });
    }
}

fn rest_base_url() -> String {
    std::env::var("WHISPER_API_BASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

fn ws_base_url() -> String {
    std::env::var("WHISPER_WS_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_WS_URL.to_string())
}

fn signed_request(
    signer: &OopzSigner,
    session: &LocalOopzSession,
    method: &str,
    path_and_query: &str,
    body_str: Option<&str>,
) -> Result<Value, String> {
    let body_for_sign = match body_str {
        Some(b) => b,
        None if matches!(method, "POST" | "PUT" | "PATCH") => "{}",
        None => "",
    };
    let headers = signer.build_headers(session, path_and_query, body_for_sign, None);
    let url = format!("{}{}", rest_base_url(), path_and_query);
    let resp = http_request(
        method,
        &url,
        &headers,
        if body_for_sign.is_empty() {
            None
        } else {
            Some(body_for_sign.as_bytes())
        },
        10_000,
    )?;
    if resp.status == 401 || resp.status == 428 {
        return Err("Oopz 登录态已失效，请重新登录 Oopz。".to_string());
    }
    if resp.status != 200 {
        return Err(format!("Oopz 请求失败 (HTTP {})", resp.status));
    }
    unwrap_api_data(&resp.body)
}

fn resolve_names(
    signer: &OopzSigner,
    session: &LocalOopzSession,
    messages: &[RawMessagePayload],
    cache: &Mutex<HashMap<String, String>>,
) {
    let missing: Vec<String> = {
        let guard = cache.lock().unwrap();
        let mut set = HashSet::new();
        for m in messages {
            if !m.sender_id.is_empty() && !guard.contains_key(&m.sender_id) {
                set.insert(m.sender_id.clone());
            }
        }
        set.into_iter().collect()
    };
    if missing.is_empty() {
        return;
    }
    for chunk in missing.chunks(30) {
        let body = json!({ "persons": chunk, "commonIds": [] }).to_string();
        if let Ok(Value::Array(users)) = signed_request(
            signer,
            session,
            "POST",
            "/client/v1/person/v1/personInfos",
            Some(&body),
        ) {
            let mut guard = cache.lock().unwrap();
            for u in users {
                let uid = u.get("uid").and_then(|v| v.as_str()).unwrap_or("");
                let name = u.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if !uid.is_empty() && !name.is_empty() {
                    guard.insert(uid.to_string(), name.to_string());
                }
            }
        }
    }
}

fn emit_to_ui(notify_hwnd: usize, tx: &mpsc::Sender<GatewayEvent>, ev: GatewayEvent) {
    let _ = tx.send(ev);
    if notify_hwnd != 0 {
        unsafe {
            let _ = PostMessageW(
                HWND(notify_hwnd as *mut _),
                WM_APP_GATEWAY_EVENT,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}
