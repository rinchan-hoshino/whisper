use crate::core::protocol::LocalOopzSession;

pub const WAITING_FOR_OOPZ_STATUS: &str = "等待 Oopz 启动…";
pub const WAITING_FOR_LOGIN_NOTICE: &str = "等待 Oopz 登录…";
pub const RECONNECTING_NOTICE: &str = "连接中断，正在重连…";
pub const CONNECTING_NOTICE: &str = "正在连接…";

/// Grace window (in seconds) during which a periodic poll with the same session
/// will not cancel and restart an in-flight WebSocket connect attempt.
pub const CONNECTING_GRACE_SECS: f64 = 10.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionProbe {
    Ready(LocalOopzSession),
    OopzNotRunning,
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleAction {
    Disconnect,
    FetchAreas(LocalOopzSession),
    ConnectChannel(LocalOopzSession),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LifecycleUpdate {
    pub actions: Vec<LifecycleAction>,
    pub hud_dirty: bool,
    pub settings_dirty: bool,
}

#[derive(Debug, Clone)]
pub struct ConnectionLifecycle {
    pub session: Option<LocalOopzSession>,
    pub connected: bool,
    pub oopz_out: bool,
    pub has_active_gateway: bool,
    pub connecting_since: Option<f64>,
    pub oopz_status_text: String,
    pub notice_text: Option<String>,
}

impl Default for ConnectionLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionLifecycle {
    pub fn new() -> Self {
        Self {
            session: None,
            connected: false,
            oopz_out: false,
            has_active_gateway: false,
            connecting_since: None,
            oopz_status_text: "正在检测本机 Oopz 登录态…".to_string(),
            notice_text: None,
        }
    }

    pub fn on_session_probe(
        &mut self,
        probe: SessionProbe,
        now_secs: f64,
        area_id: &str,
        area_name: &str,
        channel_id: &str,
        has_areas: bool,
    ) -> LifecycleUpdate {
        let mut out = LifecycleUpdate::default();
        match probe {
            SessionProbe::Ready(sess) => {
                let changed = self.session.as_ref() != Some(&sess);
                let was_oopz_out = self.oopz_out;
                let has_target_channel = !area_id.is_empty() && !channel_id.is_empty();
                let in_flight_connect = self
                    .connecting_since
                    .map(|t| (now_secs - t) < CONNECTING_GRACE_SECS)
                    .unwrap_or(false);

                self.oopz_out = false;
                self.session = Some(sess.clone());

                if changed || !has_areas {
                    out.actions.push(LifecycleAction::FetchAreas(sess.clone()));
                }

                let should_connect = has_target_channel
                    && (changed
                        || (!self.connected
                            && (was_oopz_out
                                || (!self.has_active_gateway && !in_flight_connect))));

                if should_connect {
                    self.connected = false;
                    self.has_active_gateway = true;
                    self.connecting_since = Some(now_secs);
                    self.notice_text = Some(CONNECTING_NOTICE.to_string());
                    out.actions.push(LifecycleAction::ConnectChannel(sess));
                    out.hud_dirty = true;
                    out.settings_dirty = true;
                }

                if changed || was_oopz_out || should_connect {
                    let next_status = if self.connected && !area_name.is_empty() {
                        format!("已连接 Oopz · {area_name}")
                    } else if area_name.is_empty() {
                        "已连接 Oopz · 正在拉取服务器…".to_string()
                    } else {
                        format!("已连接 Oopz · {area_name}")
                    };
                    if self.oopz_status_text != next_status {
                        self.oopz_status_text = next_status;
                        out.settings_dirty = true;
                    }
                    if was_oopz_out && !should_connect && self.notice_text.is_some() {
                        self.notice_text = None;
                        out.hud_dirty = true;
                    }
                }
            }
            SessionProbe::OopzNotRunning => {
                let need_disconnect = self.has_active_gateway || self.connected;
                let state_changing = need_disconnect
                    || !self.oopz_out
                    || self.oopz_status_text != WAITING_FOR_OOPZ_STATUS
                    || self.notice_text.as_deref() != Some(WAITING_FOR_OOPZ_STATUS);

                if need_disconnect {
                    out.actions.push(LifecycleAction::Disconnect);
                }
                if state_changing {
                    self.connected = false;
                    self.has_active_gateway = false;
                    self.oopz_out = true;
                    self.connecting_since = None;
                    self.oopz_status_text = WAITING_FOR_OOPZ_STATUS.to_string();
                    self.notice_text = Some(WAITING_FOR_OOPZ_STATUS.to_string());
                    out.hud_dirty = true;
                    out.settings_dirty = true;
                }
            }
            SessionProbe::Unavailable(err) => {
                let need_disconnect = self.has_active_gateway || self.connected;
                if need_disconnect {
                    out.actions.push(LifecycleAction::Disconnect);
                }
                let state_changing = need_disconnect
                    || !self.oopz_out
                    || self.oopz_status_text != err
                    || self.notice_text.as_deref() != Some(WAITING_FOR_LOGIN_NOTICE);
                if state_changing {
                    self.connected = false;
                    self.has_active_gateway = false;
                    self.oopz_out = true;
                    self.connecting_since = None;
                    self.oopz_status_text = err;
                    self.notice_text = Some(WAITING_FOR_LOGIN_NOTICE.to_string());
                    out.hud_dirty = true;
                    out.settings_dirty = true;
                }
            }
        }
        out
    }

    pub fn on_status_changed(
        &mut self,
        connected: bool,
        text: String,
        area_name: &str,
    ) -> LifecycleUpdate {
        self.connected = connected;
        if connected {
            self.notice_text = None;
            self.connecting_since = None;
            self.oopz_out = false;
            self.has_active_gateway = true;
            self.oopz_status_text = if area_name.is_empty() {
                "已连接 Oopz".to_string()
            } else {
                format!("已连接 Oopz · {area_name}")
            };
        } else {
            self.connecting_since = None;
            if self.oopz_out {
                self.oopz_status_text = WAITING_FOR_OOPZ_STATUS.to_string();
                self.notice_text = Some(WAITING_FOR_OOPZ_STATUS.to_string());
            } else {
                let notice = if text.trim().is_empty() {
                    RECONNECTING_NOTICE.to_string()
                } else {
                    text
                };
                self.notice_text = Some(notice);
                self.oopz_status_text = RECONNECTING_NOTICE.to_string();
            }
        }
        LifecycleUpdate {
            actions: Vec::new(),
            hud_dirty: true,
            settings_dirty: true,
        }
    }

    pub fn on_gateway_error(&mut self, err: String, auth_rejected: bool) -> LifecycleUpdate {
        if auth_rejected {
            self.connected = false;
            self.has_active_gateway = false;
            self.connecting_since = None;
            self.oopz_out = true;
        }
        self.notice_text = Some(err.clone());
        self.oopz_status_text = err;
        LifecycleUpdate {
            actions: Vec::new(),
            hud_dirty: true,
            settings_dirty: true,
        }
    }

    pub fn on_manual_connect(&mut self, now_secs: f64) {
        self.connected = false;
        self.has_active_gateway = true;
        self.oopz_out = false;
        self.connecting_since = Some(now_secs);
        self.notice_text = Some(CONNECTING_NOTICE.to_string());
    }

    pub fn on_area_cleared(&mut self, area_name: &str) {
        self.connected = false;
        self.has_active_gateway = false;
        self.connecting_since = None;
        self.notice_text = None;
        self.oopz_status_text = format!("已选择服务器 · {area_name}");
    }
}

/// Concise amber status text for the Active/Adjust HUD top bar when disconnected
/// or carrying an active notice. Returns `None` when connected and healthy.
pub fn top_bar_status_label(connected: bool, notice_text: Option<&str>) -> Option<String> {
    if let Some(raw) = notice_text.map(str::trim).filter(|s| !s.is_empty()) {
        if raw.contains("等待 Oopz") || raw.contains("尚未运行") {
            return Some("等待 Oopz…".to_string());
        }
        if raw.contains("重连") {
            return Some("正在重连…".to_string());
        }
        if raw.contains("正在连接") {
            return Some("正在连接…".to_string());
        }
        if raw.contains("失效") {
            return Some("登录态已失效".to_string());
        }
        return Some(raw.to_string());
    }
    if !connected {
        return Some("正在重连…".to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_session(token: &str) -> LocalOopzSession {
        LocalOopzSession {
            person_uid: "u1".to_string(),
            jwt_token: token.to_string(),
            device_id: "dev-1".to_string(),
            app_version: "1.2.3".to_string(),
        }
    }

    #[test]
    fn status_changed_connected_true_clears_notice_text() {
        let mut lc = ConnectionLifecycle::new();
        let sess = sample_session("tok-1");

        let u1 = lc.on_session_probe(SessionProbe::Ready(sess.clone()), 1.0, "a1", "战术屋", "c1", false);
        assert!(u1.actions.contains(&LifecycleAction::FetchAreas(sess.clone())));
        assert!(u1.actions.contains(&LifecycleAction::ConnectChannel(sess)));
        assert_eq!(lc.notice_text.as_deref(), Some(CONNECTING_NOTICE));

        // Simulate disconnect notice
        lc.on_status_changed(false, RECONNECTING_NOTICE.to_string(), "战术屋");
        assert!(!lc.connected);
        assert_eq!(lc.notice_text.as_deref(), Some(RECONNECTING_NOTICE));

        // Reconnect succeeds -> notice_text MUST be cleared!
        lc.on_status_changed(true, "#大厅".to_string(), "战术屋");
        assert!(lc.connected);
        assert_eq!(lc.notice_text, None);
        assert_eq!(lc.oopz_status_text, "已连接 Oopz · 战术屋");
        assert_eq!(top_bar_status_label(lc.connected, lc.notice_text.as_deref()), None);
    }

    #[test]
    fn oopz_exit_and_relaunch_with_same_token_reconnects_immediately() {
        let mut lc = ConnectionLifecycle::new();
        let sess = sample_session("tok-same");

        // 1. Initial login and connect
        let u1 = lc.on_session_probe(SessionProbe::Ready(sess.clone()), 0.0, "a1", "战术屋", "c1", true);
        assert_eq!(
            u1.actions,
            vec![
                LifecycleAction::FetchAreas(sess.clone()),
                LifecycleAction::ConnectChannel(sess.clone()),
            ]
        );
        lc.on_status_changed(true, "#大厅".to_string(), "战术屋");
        assert!(lc.connected);
        assert_eq!(lc.notice_text, None);

        // 2. Oopz exits while connected -> disconnects immediately & marks waiting
        let u2 = lc.on_session_probe(SessionProbe::OopzNotRunning, 5.0, "a1", "战术屋", "c1", true);
        assert_eq!(u2.actions, vec![LifecycleAction::Disconnect]);
        assert!(!lc.connected);
        assert!(lc.oopz_out);
        assert!(!lc.has_active_gateway);
        assert_eq!(lc.oopz_status_text, WAITING_FOR_OOPZ_STATUS);
        assert_eq!(
            top_bar_status_label(lc.connected, lc.notice_text.as_deref()).as_deref(),
            Some("等待 Oopz…")
        );

        // 3. Subsequent poll while Oopz is still closed is a quiet no-op
        let u3 = lc.on_session_probe(SessionProbe::OopzNotRunning, 7.5, "a1", "战术屋", "c1", true);
        assert!(u3.actions.is_empty());
        assert!(!u3.hud_dirty);
        assert!(!u3.settings_dirty);

        // 4. Oopz relaunches with the EXACT SAME token -> reconnects immediately!
        let u4 = lc.on_session_probe(SessionProbe::Ready(sess.clone()), 10.0, "a1", "战术屋", "c1", true);
        assert_eq!(u4.actions, vec![LifecycleAction::ConnectChannel(sess.clone())]);
        assert!(!lc.oopz_out);
        assert!(lc.has_active_gateway);

        // 5. Duplicate poll 2.5s later while handshake is in-flight does NOT re-trigger
        let u5 = lc.on_session_probe(SessionProbe::Ready(sess), 12.5, "a1", "战术屋", "c1", true);
        assert!(u5.actions.is_empty());

        // 6. Handshake completes -> notice cleared, connected restored
        lc.on_status_changed(true, "#大厅".to_string(), "战术屋");
        assert!(lc.connected);
        assert_eq!(lc.notice_text, None);
        assert_eq!(top_bar_status_label(lc.connected, lc.notice_text.as_deref()), None);
    }

    #[test]
    fn oopz_relaunch_with_new_token_fetches_areas_and_reconnects() {
        let mut lc = ConnectionLifecycle::new();
        let sess1 = sample_session("tok-old");
        let sess2 = sample_session("tok-new");

        lc.on_session_probe(SessionProbe::Ready(sess1), 0.0, "a1", "战术屋", "c1", true);
        lc.on_status_changed(true, "#大厅".to_string(), "战术屋");

        // Oopz exits
        let exit = lc.on_session_probe(SessionProbe::OopzNotRunning, 4.0, "a1", "战术屋", "c1", true);
        assert_eq!(exit.actions, vec![LifecycleAction::Disconnect]);

        // Oopz relaunches with new token -> fetches areas + connects channel with new token
        let relogin = lc.on_session_probe(SessionProbe::Ready(sess2.clone()), 8.0, "a1", "战术屋", "c1", true);
        assert_eq!(
            relogin.actions,
            vec![
                LifecycleAction::FetchAreas(sess2.clone()),
                LifecycleAction::ConnectChannel(sess2),
            ]
        );

        lc.on_status_changed(true, "#大厅".to_string(), "战术屋");
        assert!(lc.connected);
        assert_eq!(lc.notice_text, None);
    }

    #[test]
    fn oopz_exit_during_reconnect_loop_stops_backoff_and_recovers_cleanly() {
        let mut lc = ConnectionLifecycle::new();
        let sess = sample_session("tok-1");

        lc.on_session_probe(SessionProbe::Ready(sess.clone()), 0.0, "a1", "战术屋", "c1", true);
        lc.on_status_changed(true, "#大厅".to_string(), "战术屋");

        // WS drops first (e.g. server closed socket when Oopz started shutting down)
        lc.on_status_changed(false, RECONNECTING_NOTICE.to_string(), "战术屋");
        assert!(!lc.connected);
        assert!(lc.has_active_gateway);
        assert_eq!(
            top_bar_status_label(lc.connected, lc.notice_text.as_deref()).as_deref(),
            Some("正在重连…")
        );

        // Next poll sees Oopz exited -> actively calls Disconnect to stop backoff loop
        let exit = lc.on_session_probe(SessionProbe::OopzNotRunning, 2.5, "a1", "战术屋", "c1", true);
        assert_eq!(exit.actions, vec![LifecycleAction::Disconnect]);
        assert!(!lc.has_active_gateway);
        assert!(lc.oopz_out);

        // Oopz restarts -> immediate reconnect
        let back = lc.on_session_probe(SessionProbe::Ready(sess.clone()), 5.0, "a1", "战术屋", "c1", true);
        assert_eq!(back.actions, vec![LifecycleAction::ConnectChannel(sess)]);

        lc.on_status_changed(true, "#大厅".to_string(), "战术屋");
        assert!(lc.connected);
        assert_eq!(lc.notice_text, None);
    }
}
