use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts, WinHttpWebSocketClose,
    WinHttpWebSocketCompleteUpgrade, WinHttpWebSocketReceive, WinHttpWebSocketSend,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_ADDREQ_FLAG_REPLACE,
    WINHTTP_FLAG_SECURE, WINHTTP_OPTION_UPGRADE_TO_WEB_SOCKET, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WINHTTP_WEB_SOCKET_BUFFER_TYPE,
    WINHTTP_WEB_SOCKET_CLOSE_BUFFER_TYPE, WINHTTP_WEB_SOCKET_SUCCESS_CLOSE_STATUS,
    WINHTTP_WEB_SOCKET_UTF8_FRAGMENT_BUFFER_TYPE, WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE,
};

use crate::platform::to_wide_null;

#[derive(Debug, Clone)]
pub struct ParsedUrl {
    pub is_secure: bool,
    pub host: String,
    pub port: u16,
    pub path_and_query: String,
}

pub fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let (is_secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else if let Some(r) = url.strip_prefix("wss://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("ws://") {
        (false, r)
    } else {
        return Err(format!("unsupported URL scheme: {url}"));
    };

    let (authority, path_and_query) = match rest.find('/') {
        Some(idx) => (&rest[..idx], rest[idx..].to_string()),
        None => (rest, "/".to_string()),
    };

    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => {
            let port = p
                .parse::<u16>()
                .map_err(|_| format!("invalid port in URL: {url}"))?;
            (h.to_string(), port)
        }
        None => (authority.to_string(), if is_secure { 443 } else { 80 }),
    };

    Ok(ParsedUrl {
        is_secure,
        host,
        port,
        path_and_query,
    })
}

pub struct HttpResponse {
    pub status: u32,
    pub body: Vec<u8>,
}

struct WinHttpHandle(*mut c_void);

impl Drop for WinHttpHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

pub fn http_request(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    timeout_ms: i32,
) -> Result<HttpResponse, String> {
    let parsed = parse_url(url)?;
    let ua = to_wide_null("Whisper/2.0.0");
    let host_w = to_wide_null(&parsed.host);
    let method_w = to_wide_null(method);
    let path_w = to_wide_null(&parsed.path_and_query);

    unsafe {
        let session = WinHttpOpen(
            PCWSTR(ua.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return Err("WinHttpOpen failed".to_string());
        }
        let _session = WinHttpHandle(session);
        let _ = WinHttpSetTimeouts(session, timeout_ms, timeout_ms, timeout_ms, timeout_ms);

        let connect = WinHttpConnect(session, PCWSTR(host_w.as_ptr()), parsed.port, 0);
        if connect.is_null() {
            return Err(format!("WinHttpConnect failed for {}", parsed.host));
        }
        let _connect = WinHttpHandle(connect);

        let flags = if parsed.is_secure {
            WINHTTP_FLAG_SECURE
        } else {
            windows::Win32::Networking::WinHttp::WINHTTP_OPEN_REQUEST_FLAGS(0)
        };
        let request = WinHttpOpenRequest(
            connect,
            PCWSTR(method_w.as_ptr()),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        );
        if request.is_null() {
            return Err("WinHttpOpenRequest failed".to_string());
        }
        let _request = WinHttpHandle(request);

        for (k, v) in headers {
            let line = to_wide_null(&format!("{k}: {v}\r\n"));
            let _ = WinHttpAddRequestHeaders(
                request,
                &line[..line.len() - 1],
                WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE,
            );
        }

        let (body_ptr, body_len) = match body {
            Some(b) if !b.is_empty() => (Some(b.as_ptr() as *const c_void), b.len() as u32),
            _ => (None, 0),
        };

        WinHttpSendRequest(request, None, body_ptr, body_len, body_len, 0)
            .map_err(|e| format!("WinHttpSendRequest failed: {e}"))?;
        WinHttpReceiveResponse(request, std::ptr::null_mut())
            .map_err(|e| format!("WinHttpReceiveResponse failed: {e}"))?;

        let mut status: u32 = 0;
        let mut status_len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut status_len,
            std::ptr::null_mut(),
        )
        .map_err(|e| format!("WinHttpQueryHeaders status failed: {e}"))?;

        let mut out = Vec::new();
        loop {
            let mut avail: u32 = 0;
            WinHttpQueryDataAvailable(request, &mut avail)
                .map_err(|e| format!("WinHttpQueryDataAvailable failed: {e}"))?;
            if avail == 0 {
                break;
            }
            let start = out.len();
            out.resize(start + avail as usize, 0);
            let mut read: u32 = 0;
            WinHttpReadData(
                request,
                out[start..].as_mut_ptr() as *mut c_void,
                avail,
                &mut read,
            )
            .map_err(|e| format!("WinHttpReadData failed: {e}"))?;
            out.truncate(start + read as usize);
            if read == 0 {
                break;
            }
        }

        Ok(HttpResponse { status, body: out })
    }
}

pub struct WinHttpWebSocket {
    _session: WinHttpHandle,
    _connect: WinHttpHandle,
    ws: *mut c_void,
}

unsafe impl Send for WinHttpWebSocket {}
unsafe impl Sync for WinHttpWebSocket {}

impl WinHttpWebSocket {
    pub fn connect(url: &str, headers: &[(String, String)]) -> Result<Self, String> {
        let parsed = parse_url(url)?;
        let ua = to_wide_null("Whisper/2.0.0");
        let host_w = to_wide_null(&parsed.host);
        let method_w = to_wide_null("GET");
        let path_w = to_wide_null(&parsed.path_and_query);

        unsafe {
            let session = WinHttpOpen(
                PCWSTR(ua.as_ptr()),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            );
            if session.is_null() {
                return Err("WinHttpOpen failed".to_string());
            }
            let session_guard = WinHttpHandle(session);
            let _ = WinHttpSetTimeouts(session, 10000, 10000, 10000, 15000);

            let connect = WinHttpConnect(session, PCWSTR(host_w.as_ptr()), parsed.port, 0);
            if connect.is_null() {
                return Err(format!("WinHttpConnect failed for {}", parsed.host));
            }
            let connect_guard = WinHttpHandle(connect);

            let flags = if parsed.is_secure {
                WINHTTP_FLAG_SECURE
            } else {
                windows::Win32::Networking::WinHttp::WINHTTP_OPEN_REQUEST_FLAGS(0)
            };
            let request = WinHttpOpenRequest(
                connect,
                PCWSTR(method_w.as_ptr()),
                PCWSTR(path_w.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                flags,
            );
            if request.is_null() {
                return Err("WinHttpOpenRequest failed".to_string());
            }
            let request_guard = WinHttpHandle(request);

            for (k, v) in headers {
                let line = to_wide_null(&format!("{k}: {v}\r\n"));
                let _ = WinHttpAddRequestHeaders(
                    request,
                    &line[..line.len() - 1],
                    WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE,
                );
            }

            WinHttpSetOption(
                Some(request as *const c_void),
                WINHTTP_OPTION_UPGRADE_TO_WEB_SOCKET,
                None,
            )
                .map_err(|e| format!("WinHttpSetOption WS upgrade failed: {e}"))?;

            WinHttpSendRequest(request, None, None, 0, 0, 0)
                .map_err(|e| format!("WS handshake send failed: {e}"))?;
            WinHttpReceiveResponse(request, std::ptr::null_mut())
                .map_err(|e| format!("WS handshake receive failed: {e}"))?;

            let ws = WinHttpWebSocketCompleteUpgrade(request, 0);
            drop(request_guard);
            if ws.is_null() {
                return Err("WinHttpWebSocketCompleteUpgrade failed".to_string());
            }

            Ok(Self {
                _session: session_guard,
                _connect: connect_guard,
                ws,
            })
        }
    }

    pub fn send_text(&self, text: &str) -> Result<(), String> {
        let bytes = text.as_bytes();
        let err = unsafe {
            WinHttpWebSocketSend(
                self.ws,
                WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE,
                Some(bytes),
            )
        };
        if err != 0 {
            return Err(format!("WinHttpWebSocketSend error: {err}"));
        }
        Ok(())
    }

    pub fn recv_text(&self) -> Result<Option<String>, String> {
        let mut msg = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let mut bytes_read: u32 = 0;
            let mut buf_type = WINHTTP_WEB_SOCKET_BUFFER_TYPE(0);
            let err = unsafe {
                WinHttpWebSocketReceive(
                    self.ws,
                    buf.as_mut_ptr() as *mut c_void,
                    buf.len() as u32,
                    &mut bytes_read,
                    &mut buf_type,
                )
            };
            if err != 0 {
                return Err(format!("WinHttpWebSocketReceive error: {err}"));
            }
            if buf_type == WINHTTP_WEB_SOCKET_CLOSE_BUFFER_TYPE {
                return Ok(None);
            }
            msg.extend_from_slice(&buf[..bytes_read as usize]);
            if buf_type == WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE {
                return Ok(Some(String::from_utf8_lossy(&msg).into_owned()));
            }
            if buf_type != WINHTTP_WEB_SOCKET_UTF8_FRAGMENT_BUFFER_TYPE {
                return Ok(Some(String::from_utf8_lossy(&msg).into_owned()));
            }
        }
    }

    pub fn close(&self) {
        if !self.ws.is_null() {
            unsafe {
                let _ = WinHttpWebSocketClose(
                    self.ws,
                    WINHTTP_WEB_SOCKET_SUCCESS_CLOSE_STATUS.0 as u16,
                    None,
                    0,
                );
            }
        }
    }
}

impl Drop for WinHttpWebSocket {
    fn drop(&mut self) {
        if !self.ws.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.ws);
            }
            self.ws = std::ptr::null_mut();
        }
    }
}
