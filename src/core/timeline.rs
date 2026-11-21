use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub message_id: String,
    pub timestamp_us: i64,
    pub sender_id: String,
    pub sender_name: String,
    pub text: String,
    pub mine: bool,
}

/// Deterministic timeline store with deduplication, 80-message cap,
/// 8s hold + 2s fade-out opacity curve, edit/delete, and scrollback unread tracking.
#[derive(Debug, Clone)]
pub struct Timeline {
    limit: usize,
    pub unread: usize,
    messages: HashMap<String, ChatMessage>,
    seen: HashSet<String>,
    arrivals: HashMap<String, f64>,
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new(80)
    }
}

impl Timeline {
    pub fn new(limit: usize) -> Self {
        assert!(limit >= 1, "timeline limit must be positive");
        Self {
            limit,
            unread: 0,
            messages: HashMap::new(),
            seen: HashSet::new(),
            arrivals: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.seen.clear();
        self.arrivals.clear();
        self.unread = 0;
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Returns opacity in [0.0, 1.0]: 1.0 for 0..=8s, linearly fading to 0.0 over 8..=10s.
    pub fn opacity(&self, message_id: &str, now_secs: f64) -> f32 {
        let arrival = self.arrivals.get(message_id).copied().unwrap_or(now_secs - 12.0);
        let age = now_secs - arrival;
        if age <= 8.0 {
            1.0
        } else if age >= 10.0 {
            0.0
        } else {
            ((10.0 - age) / 2.0) as f32
        }
    }

    /// Whether any message is currently in the 0..10s live visibility/fade window.
    pub fn has_active_fade(&self, now_secs: f64) -> bool {
        self.arrivals.values().any(|&t| {
            let age = now_secs - t;
            age >= 0.0 && age < 10.2
        })
    }

    /// Chronologically sorted messages (oldest first).
    pub fn items(&self) -> Vec<ChatMessage> {
        let mut list: Vec<ChatMessage> = self.messages.values().cloned().collect();
        list.sort_by(|a, b| {
            a.timestamp_us
                .cmp(&b.timestamp_us)
                .then_with(|| a.message_id.cmp(&b.message_id))
        });
        list
    }

    /// Merge history or live messages. Returns message IDs that should trigger a sound notification.
    pub fn merge(
        &mut self,
        incoming: impl IntoIterator<Item = ChatMessage>,
        now_secs: f64,
        live: bool,
        reading_bottom: bool,
    ) -> Vec<String> {
        let mut notify = Vec::new();
        for msg in incoming {
            if msg.message_id.is_empty() {
                continue;
            }
            let key = msg.message_id.clone();
            if !self.seen.contains(&key) {
                self.seen.insert(key.clone());
                self.arrivals
                    .insert(key.clone(), if live { now_secs } else { now_secs - 12.0 });
                if live && !msg.mine {
                    notify.push(key.clone());
                }
            }
            self.messages.insert(key, msg);
        }

        if self.messages.len() > self.limit {
            let sorted = self.items();
            let keep_from = sorted.len().saturating_sub(self.limit);
            let mut next_map = HashMap::with_capacity(self.limit);
            for m in sorted.into_iter().skip(keep_from) {
                next_map.insert(m.message_id.clone(), m);
            }
            self.messages = next_map;
            self.seen.retain(|k| self.messages.contains_key(k));
            self.arrivals.retain(|k, _| self.messages.contains_key(k));
        }

        notify.retain(|k| self.messages.contains_key(k));
        if !reading_bottom {
            self.unread += notify.len();
        }
        notify
    }

    pub fn remove_message(&mut self, message_id: &str) -> bool {
        let removed = self.messages.remove(message_id).is_some();
        self.seen.remove(message_id);
        self.arrivals.remove(message_id);
        removed
    }

    pub fn mark_read(&mut self) {
        self.unread = 0;
    }

    /// Stable pleasant color palette index for a sender UID.
    pub fn sender_color_rgb(sender_id: &str) -> (u8, u8, u8) {
        const PALETTE: [(u8, u8, u8); 8] = [
            (0x38, 0xBD, 0xF8), // Sky cyan (matches 匣子 in mockup)
            (0xFA, 0xCC, 0x15), // Warm yellow (matches 凉雨 in mockup)
            (0x4A, 0xDE, 0x80), // Fresh green (matches 黑夜 in mockup)
            (0xFB, 0x92, 0x3C), // Soft orange
            (0xC0, 0x84, 0xFC), // Soft lavender
            (0xF4, 0x72, 0xB6), // Soft pink
            (0x2D, 0xD4, 0xBF), // Teal mint
            (0x93, 0xC5, 0xFD), // Periwinkle blue
        ];
        // FNV-1a 32-bit hash for stable cross-platform mapping
        let mut h: u32 = 2166136261;
        for b in sender_id.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(16777619);
        }
        PALETTE[(h as usize) % PALETTE.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, ts: i64, mine: bool) -> ChatMessage {
        ChatMessage {
            message_id: id.to_string(),
            timestamp_us: ts,
            sender_id: if mine { "me" } else { "u1" }.to_string(),
            sender_name: if mine { "我" } else { "队友" }.to_string(),
            text: format!("msg {id}"),
            mine,
        }
    }

    #[test]
    fn dedup_and_cap_and_unread() {
        let mut tl = Timeline::new(3);
        let n1 = tl.merge(vec![msg("a", 10, false), msg("b", 20, true)], 100.0, true, false);
        assert_eq!(n1, vec!["a"]);
        assert_eq!(tl.unread, 1);

        // Duplicate live message does not re-notify or double unread
        let n2 = tl.merge(vec![msg("a", 10, false)], 101.0, true, false);
        assert!(n2.is_empty());
        assert_eq!(tl.unread, 1);

        // Cap at 3 trims oldest
        tl.merge(vec![msg("c", 30, false), msg("d", 40, false)], 102.0, true, true);
        let ids: Vec<String> = tl.items().into_iter().map(|m| m.message_id).collect();
        assert_eq!(ids, vec!["b", "c", "d"]);
        // reading_bottom = true so unread did not increment
        assert_eq!(tl.unread, 1);

        tl.mark_read();
        assert_eq!(tl.unread, 0);
    }

    #[test]
    fn fade_opacity_curve() {
        let mut tl = Timeline::new(10);
        tl.merge(vec![msg("x", 1, false)], 50.0, true, true);
        assert!((tl.opacity("x", 50.0) - 1.0).abs() < 1e-5);
        assert!((tl.opacity("x", 58.0) - 1.0).abs() < 1e-5);
        assert!((tl.opacity("x", 59.0) - 0.5).abs() < 1e-5);
        assert!((tl.opacity("x", 60.0) - 0.0).abs() < 1e-5);
        assert!(!tl.has_active_fade(61.0));
    }

    #[test]
    fn edit_and_delete_behavior() {
        let mut tl = Timeline::new(10);
        tl.merge(vec![msg("m1", 10, false)], 10.0, true, true);
        let mut edited = msg("m1", 10, false);
        edited.text = "updated text".to_string();
        let notify = tl.merge(vec![edited], 12.0, true, false);
        assert!(notify.is_empty());
        assert_eq!(tl.items()[0].text, "updated text");
        assert!(tl.remove_message("m1"));
        assert!(tl.is_empty());
    }
}
