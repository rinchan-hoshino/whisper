/// Inline micro-capsule extraction for URLs and 4-10 char alphanumeric room/invite codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSegment {
    Plain(String),
    Url {
        display: String,
        copy_value: String,
    },
    RoomCode {
        code: String,
    },
}

/// Extract URLs (`https://...`, `http://...`, `www....`) and standalone 4-10 char
/// alphanumeric room/invite codes (must contain at least one digit OR be all-uppercase
/// alphanumeric like `QILZU3`, `ABCD12`, `884210`, avoiding ordinary lowercase English words).
pub fn parse_segments(text: &str) -> Vec<TextSegment> {
    let mut out: Vec<TextSegment> = Vec::new();
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut plain_start = 0;

    while i < len {
        // 1. Check URL start
        if let Some(url_end) = match_url(text, i) {
            if i > plain_start {
                out.push(TextSegment::Plain(text[plain_start..i].to_string()));
            }
            let raw_url = &text[i..url_end];
            let copy_value = if raw_url.starts_with("http://") || raw_url.starts_with("https://") {
                raw_url.to_string()
            } else {
                format!("https://{raw_url}")
            };
            out.push(TextSegment::Url {
                display: shorten_url(raw_url),
                copy_value,
            });
            i = url_end;
            plain_start = i;
            continue;
        }

        // 2. Check standalone ASCII alphanumeric token for room/invite code
        if bytes[i].is_ascii_alphanumeric() {
            let prev_ok = i == 0 || is_code_boundary_before(text, i);
            let mut j = i;
            while j < len && bytes[j].is_ascii_alphanumeric() {
                j += 1;
            }
            let next_ok = j == len || is_code_boundary_after(text, j);
            let token = &text[i..j];
            if prev_ok && next_ok && is_room_code(token) {
                let emit_end = if i > plain_start && bytes[i - 1] == b'#' {
                    i - 1
                } else {
                    i
                };
                if emit_end > plain_start {
                    out.push(TextSegment::Plain(text[plain_start..emit_end].to_string()));
                }
                out.push(TextSegment::RoomCode {
                    code: token.to_string(),
                });
                i = j;
                plain_start = i;
                continue;
            }
            i = j;
            continue;
        }

        // Advance by one UTF-8 character
        if let Some(ch) = text[i..].chars().next() {
            i += ch.len_utf8();
        } else {
            break;
        }
    }

    if plain_start < len {
        out.push(TextSegment::Plain(text[plain_start..].to_string()));
    }

    if out.is_empty() {
        out.push(TextSegment::Plain(String::new()));
    }
    out
}

fn match_url(text: &str, start: usize) -> Option<usize> {
    let rest = &text[start..];
    let prefix_len = if rest.starts_with("https://") {
        8
    } else if rest.starts_with("http://") {
        7
    } else if rest.starts_with("www.") {
        4
    } else {
        return None;
    };

    // Require boundary before URL
    if start > 0 {
        let prev = text[..start].chars().next_back()?;
        if prev.is_ascii_alphanumeric() {
            return None;
        }
    }

    let mut end = start + prefix_len;
    for (rel, ch) in text[end..].char_indices() {
        if ch.is_whitespace()
            || matches!(
                ch,
                '<' | '>' | '"' | '\'' | '（' | '）' | '，' | '。' | '！' | '？' | '、' | '；' | '：'
            )
            || !ch.is_ascii()
        {
            end += rel;
            break;
        }
        end = start + prefix_len + rel + ch.len_utf8();
    }

    // Trim trailing ASCII punctuation that is usually sentence-ending
    while end > start + prefix_len {
        let last = text[..end].chars().next_back()?;
        if matches!(last, '.' | ',' | ';' | '!' | '?' | ')' | ']') {
            end -= last.len_utf8();
        } else {
            break;
        }
    }

    if end <= start + prefix_len + 2 {
        return None;
    }
    let host_part = &text[start + prefix_len..end];
    if !host_part.contains('.') {
        return None;
    }
    Some(end)
}

pub fn shorten_url(raw: &str) -> String {
    let stripped = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))
        .unwrap_or(raw);
    let stripped = stripped.strip_prefix("www.").unwrap_or(stripped);
    let max_chars = 24;
    if stripped.chars().count() <= max_chars {
        stripped.to_string()
    } else {
        let truncated: String = stripped.chars().take(max_chars - 1).collect();
        format!("{truncated}…")
    }
}

fn is_code_boundary_before(text: &str, idx: usize) -> bool {
    match text[..idx].chars().next_back() {
        None => true,
        Some(ch) => {
            !ch.is_ascii_alphanumeric() && ch != '/' && ch != '.' && ch != '_' && ch != '-'
        }
    }
}

fn is_code_boundary_after(text: &str, idx: usize) -> bool {
    match text[idx..].chars().next() {
        None => true,
        Some(ch) => !ch.is_ascii_alphanumeric() && ch != '/' && ch != '.' && ch != '_',
    }
}

fn is_room_code(token: &str) -> bool {
    let len = token.len();
    if !(4..=10).contains(&len) {
        return false;
    }
    let has_digit = token.bytes().any(|b| b.is_ascii_digit());
    let has_upper = token.bytes().any(|b| b.is_ascii_uppercase());
    let has_lower = token.bytes().any(|b| b.is_ascii_lowercase());
    let all_digits = token.bytes().all(|b| b.is_ascii_digit());

    // Pure digits: 5-10 digits (e.g. room ID 884210, avoiding 4-digit years like 2026)
    if all_digits {
        return len >= 5;
    }
    // Uppercase + digit mix (like QILZU3, CS2A99, AB12CD)
    if has_upper && has_digit && !has_lower {
        return true;
    }
    // Pure uppercase 5-8 chars (like ABCDE) or mixed case with digits (like Bv1x2y)
    if has_digit && (has_upper || len >= 6) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_url_and_room_code_capsules() {
        let segs = parse_segments("这把打完看下回放 https://www.bilibili.com/video/BV1xx411c7mD");
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0], TextSegment::Plain("这把打完看下回放 ".to_string()));
        match &segs[1] {
            TextSegment::Url { display, copy_value } => {
                assert!(display.starts_with("bilibili.com/video/"));
                assert_eq!(copy_value, "https://www.bilibili.com/video/BV1xx411c7mD");
            }
            other => panic!("expected Url, got {other:?}"),
        }

        let segs2 = parse_segments("换自定义房，密码 QILZU3 快来");
        assert_eq!(
            segs2,
            vec![
                TextSegment::Plain("换自定义房，密码 ".to_string()),
                TextSegment::RoomCode {
                    code: "QILZU3".to_string()
                },
                TextSegment::Plain(" 快来".to_string()),
            ]
        );
    }

    #[test]
    fn avoids_false_positives_on_normal_words_and_years() {
        let segs = parse_segments("hello world 2026 good game");
        assert_eq!(
            segs,
            vec![TextSegment::Plain("hello world 2026 good game".to_string())]
        );
    }

    #[test]
    fn handles_numeric_room_id_and_multiple_capsules() {
        let segs = parse_segments("房号884210 密码AB12C 链接www.oopz.cn/join");
        assert_eq!(
            segs,
            vec![
                TextSegment::Plain("房号".to_string()),
                TextSegment::RoomCode {
                    code: "884210".to_string()
                },
                TextSegment::Plain(" 密码".to_string()),
                TextSegment::RoomCode {
                    code: "AB12C".to_string()
                },
                TextSegment::Plain(" 链接".to_string()),
                TextSegment::Url {
                    display: "oopz.cn/join".to_string(),
                    copy_value: "https://www.oopz.cn/join".to_string()
                }
            ]
        );
    }
}
