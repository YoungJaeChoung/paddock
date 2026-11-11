//! 상태 줄의 AI 사용량 게이지가 쓰는 순수 계산.
//!
//! 앱이 로그인을 처리하지 않고, 각 공식 CLI가 보고한 값만 표시한다.
//! - Codex: 공식 CLI의 계정 한도 조회 또는 구버전 세션 기록(`~/.codex/sessions/…/rollout-*.jsonl`)의 `rate_limits`.
//! - Claude Code: 상태 줄(statusline) 명령이 받는 입력의 `rate_limits`(Pro·Max 구독).
//!
//! 창(window) 하나는 이름표(`5h`·`week`·…), 사용량(0~100 정수), 초기화 시각이다.
//! 7일 창은 `7d` 대신 `week`로 적는다 — 상태 줄의 작은 글자에서 `5h`와 한 글자만 달라 헷갈리지 않게 모양부터 다르게 하고,
//! 줄임말(`wk`)은 뜻이 바로 안 읽혀 온전한 단어로 쓴다.
use chrono::{Local, TimeZone};
use serde_json::Value;

use crate::agent_account::Linked;

/// 창이 초기화되는 시각에 대해 아는 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reset {
    /// 기록에 시각이 없었다(CLI가 보고하지 않음).
    Unknown,
    /// 시각이 지나 이미 초기화되었다. 다음 초기화 시각은 다음 요청이 새 창을 열 때 정해지므로 알 수 없다.
    Done,
    /// 초기화 시각(초 단위 Unix 시각).
    At(i64),
}

/// 사용량 창 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageWindow {
    pub label: String,
    pub used: i64,
    pub reset: Reset,
}

/// Claude 사용량 표시 설정이 기록한 상태(`usage/state.json`의 `claude`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClaudeState {
    /// 아직 정한 적 없음.
    #[default]
    Unset,
    On,
    Off,
}

impl ClaudeState {
    /// 기록 글자에서 읽는다. 모르는 글자는 `Unset`이다.
    pub fn parse(text: &str) -> Self {
        match text {
            "on" => Self::On,
            "off" => Self::Off,
            _ => Self::Unset,
        }
    }
}

/// 한 범위(계정·기본 CLI)의 사용량 읽기 결과.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub windows: Vec<UsageWindow>,
    /// 값이 기록된 시각(초 단위 Unix 시각). 모르면 None.
    pub updated_at: Option<i64>,
    /// 실시간 조회 대신 쓴 세션 기록이다.
    pub recorded: bool,
    /// Claude 사용량 표시 설정. Codex는 항상 `Unset`이다.
    pub state: ClaudeState,
    /// 기본 CLI의 로그인이 이 등록 계정 하나와 같다고 확인되었을 때만 있다.
    pub account: Option<Linked>,
    /// 읽지 못한 이유 문장(원문 출력은 담지 않는다).
    pub error: Option<String>,
}

/// 창 길이(분)의 짧은 이름표.
///
/// Examples
/// --------
/// | 분    | 이름표 |
/// |-------|--------|
/// | 300   | `5h`   |
/// | 10080 | `week` |
/// | 1440  | `1d`   |
/// | 45    | `45m`  |
pub fn window_label(minutes: i64) -> String {
    if minutes == 10080 {
        "week".to_string()
    } else if minutes % 1440 == 0 {
        format!("{}d", minutes / 1440)
    } else if minutes % 60 == 0 {
        format!("{}h", minutes / 60)
    } else {
        format!("{minutes}m")
    }
}

/// 말풍선에 쓰는 창 이름. 짧은 이름표를 풀어 쓴다.
///
/// Examples
/// --------
/// | 이름표 | 결과      |
/// |--------|-----------|
/// | `5h`   | `5-hour`  |
/// | `week` | `weekly`  |
/// | `45m`  | `45m`     |
pub fn window_name(label: &str) -> String {
    match label {
        "5h" => "5-hour".to_string(),
        "week" => "weekly".to_string(),
        other => other.to_string(),
    }
}

/// JS `Math.round`와 같은 반올림(0.5는 위로).
fn round_percent(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

/// 초기화 시각 값: 숫자면 `At`, 그 밖(빠짐·null·문자)은 `Unknown`.
fn reset_of(value: Option<&Value>) -> Reset {
    match value.and_then(Value::as_f64) {
        Some(seconds) if seconds != 0.0 => Reset::At(seconds as i64),
        _ => Reset::Unknown,
    }
}

/// 기록의 초기화 시각 값: JSON `null`은 이미 초기화된 창(`Done`), 빠졌거나 숫자가 아니면 `Unknown`, 숫자면 `At`이다.
fn record_reset(value: Option<&Value>) -> Reset {
    match value {
        Some(Value::Null) => Reset::Done,
        other => reset_of(other),
    }
}

/// Codex 세션 기록 전체 문자열에서 마지막 사용량 한도를 창별로 돌려준다(초기화가 이른 창이 먼저).
///
/// 한도 기록이 없거나 줄이 깨졌으면 그 줄은 건너뛰고, 하나도 없으면 빈 목록이다.
/// 계정 한도는 기본 묶음(`limit_id`가 `codex`, 구버전 기록은 `limit_id` 없음)만 쓴다. 다른 묶음(`premium` 등)이나
/// 창이 하나도 없는 줄(`primary`·`secondary`가 모두 null)은 건너뛰어, 그 줄이 마지막에 있어도 앞선 계정 한도가 사라지지 않는다.
///
/// Examples
/// --------
/// | 마지막 줄들                               | 결과          |
/// |-------------------------------------------|---------------|
/// | codex: week 16% → premium: 창 없음        | `week 16%`    |
/// | `{broken` · 한도 없는 줄                   | (빈 목록)     |
pub fn codex_windows(text: &str) -> Vec<UsageWindow> {
    let mut limits: Option<Value> = None;
    for line in text.lines().rev() {
        if limits.is_none() && line.contains("\"rate_limits\"") {
            let candidate = serde_json::from_str::<Value>(line)
                .ok()
                .and_then(|value| value.get("payload")?.get("rate_limits").cloned())
                .filter(|candidate| !candidate.is_null());
            if let Some(candidate) = candidate {
                let bucket = candidate.get("limit_id").and_then(Value::as_str).unwrap_or("");
                let is_default_bucket = bucket.is_empty() || bucket == "codex";
                let has_window = ["primary", "secondary"].iter().any(|name| {
                    candidate.get(name).is_some_and(|entry| !entry.is_null() && entry != &Value::Bool(false))
                });
                if is_default_bucket && has_window {
                    limits = Some(candidate);
                }
            }
        }
    }
    let mut windows: Vec<UsageWindow> = Vec::new();
    if let Some(limits) = limits {
        for name in ["primary", "secondary"] {
            let entry = limits.get(name);
            let used = entry.and_then(|entry| entry.get("used_percent")).and_then(Value::as_f64);
            let minutes = entry.and_then(|entry| entry.get("window_minutes")).and_then(Value::as_f64);
            if let (Some(used), Some(minutes)) = (used, minutes)
                && minutes != 0.0
            {
                windows.push(UsageWindow {
                    label: window_label(minutes as i64),
                    used: round_percent(used),
                    reset: record_reset(entry.and_then(|entry| entry.get("resets_at"))),
                });
            }
        }
    }
    windows.sort_by_key(|window| match window.reset {
        Reset::At(seconds) => seconds,
        Reset::Done | Reset::Unknown => i64::MAX,
    });
    windows
}

/// Claude Code 상태 줄 입력에서 5시간·주간 창을 돌려준다. 구독이 아니면 빈 목록이다.
///
/// Examples
/// --------
/// | 입력                                              | 결과                    |
/// |---------------------------------------------------|-------------------------|
/// | five_hour 23.5%, seven_day 41.2%                  | `5h 24%`, `week 41%`    |
/// | `{model: {}}`                                     | (빈 목록)               |
pub fn claude_windows(input: &Value) -> Vec<UsageWindow> {
    let mut windows = Vec::new();
    for (key, label) in [("five_hour", "5h"), ("seven_day", "week")] {
        let entry = input.get("rate_limits").and_then(|limits| limits.get(key));
        if let Some(used) = entry.and_then(|entry| entry.get("used_percentage")).and_then(Value::as_f64) {
            let reset = record_reset(entry.and_then(|entry| entry.get("resets_at")));
            windows.push(UsageWindow { label: label.to_string(), used: round_percent(used), reset });
        }
    }
    windows
}

/// 지금 기준의 창 목록. 초기화 시각이 지난 창은 빼지 않고 사용량 0%·초기화됨으로 바꾼다.
///
/// 기록은 CLI를 마지막으로 쓸 때 남으므로, 5시간 창이 초기화된 뒤 아직 안 썼으면 옛 값만 남아 있다.
/// 이 창을 빼면 한도가 있다는 사실 자체가 사라져 보여, 실제 상태(초기화되어 0%)로 보인다.
///
/// Examples
/// --------
/// | 지금 | 창                                                | 결과                                      |
/// |------|---------------------------------------------------|-------------------------------------------|
/// | 200  | `5h` 90% 초기화 100, `week` 10% 초기화 300        | `5h` 0% 초기화됨, `week` 10% 초기화 300   |
pub fn current_windows(
    windows: &[UsageWindow],
    now_seconds: i64,
) -> Vec<UsageWindow> {
    windows
        .iter()
        .map(|window| match window.reset {
            Reset::At(at) if at <= now_seconds => UsageWindow { used: 0, reset: Reset::Done, ..window.clone() },
            _ => window.clone(),
        })
        .collect()
}

/// 기록의 창 가운데 아직 초기화되지 않은 창이 있는지. 모든 창의 초기화 시각이 지났거나 시각을 모르면 false다.
///
/// 실시간 조회에 실패해 대신 쓰는 세션 기록이 지금도 의미 있는 값인지 가리려고 존재한다 — 초기화가 모두 지난 기록은
/// 지금 사용량에 대해 아무것도 확인해 주지 않는다.
///
/// Examples
/// --------
/// | 지금 | 창                   | 결과  |
/// |------|----------------------|-------|
/// | 200  | `week` 초기화 300    | true  |
/// | 200  | `week` 초기화 100    | false |
/// | 200  | `week` 시각 모름     | false |
/// | 200  | (빈 목록)            | false |
pub fn has_current_window(
    windows: &[UsageWindow],
    now_seconds: i64,
) -> bool {
    windows.iter().any(|window| matches!(window.reset, Reset::At(at) if at > now_seconds))
}

/// `3h 5m`·`2d 4h` 꼴의 남은 시간.
pub fn duration(seconds: i64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    if days > 0 { format!("{days}d {hours}h") } else { format!("{hours}h {minutes}m") }
}

/// 상태 줄에 붙이는 짧은 경과 시간. 가장 큰 단위 하나만 쓴다.
///
/// Examples
/// --------
/// | 초     | 결과  |
/// |--------|-------|
/// | 59     | `0m`  |
/// | 600    | `10m` |
/// | 67680  | `18h` |
/// | 259200 | `3d`  |
pub fn short_age(seconds: i64) -> String {
    if seconds >= 86400 {
        format!("{}d", seconds / 86400)
    } else if seconds >= 3600 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}m", seconds / 60)
    }
}

/// 창이 초기화되는 시각을 지금 시간대의 날짜·시각으로 적는다. 오늘이면 시각만, 다른 날이면 날짜를 앞에 붙인다.
/// 시각을 모르면(초기화되었거나 기록에 없음) 빈 글자다.
///
/// Examples
/// --------
/// | 초기화 시각          | 결과            |
/// |----------------------|-----------------|
/// | 오늘 18:30           | `18:30`         |
/// | 10월 15일 09:05      | `Oct 15, 09:05` |
/// | 초기화됨·모름        | (빈 글자)       |
pub fn reset_clock(
    window: &UsageWindow,
    now_seconds: i64,
) -> String {
    let mut text = String::new();
    if let Reset::At(at) = window.reset
        && at > now_seconds
        && let (Some(reset), Some(now)) =
            (Local.timestamp_opt(at, 0).single(), Local.timestamp_opt(now_seconds, 0).single())
    {
        let time = reset.format("%H:%M").to_string();
        text =
            if reset.date_naive() == now.date_naive() { time } else { format!("{}, {time}", reset.format("%b %-d")) };
    }
    text
}

/// 게이지 말풍선 문구.
///
/// Examples
/// --------
/// | 창                                  | 결과                                                                    |
/// |-------------------------------------|-------------------------------------------------------------------------|
/// | 23% 사용, 2시간 10분 뒤 초기화      | `23% used · 77% left · resets in 2h 10m`                                |
/// | 0% 사용, 초기화됨                   | `0% used · 100% left · reset — a new window starts with your next request` |
/// | 5% 사용, 시각 모름                  | `5% used · 95% left`                                                    |
pub fn describe(
    window: &UsageWindow,
    now_seconds: i64,
) -> String {
    let reset = match window.reset {
        Reset::At(at) => format!(" \u{b7} resets in {}", duration((at - now_seconds).max(0))),
        Reset::Done => " \u{b7} reset \u{2014} a new window starts with your next request".to_string(),
        Reset::Unknown => String::new(),
    };
    format!("{}% used \u{b7} {}% left{reset}", window.used, (100 - window.used).max(0))
}

/// Paddock 상태 줄 명령이 쓰는 파일 경로들.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLineFiles {
    pub script: String,
    pub usage_file: String,
    pub previous_file: String,
}

/// 큰따옴표로 묶인 인자들을 꺼낸다. Windows 경로는 구분자를 그대로 두고, POSIX 인자는 `\"`·`\\`·`\$`·`` \` `` 이스케이프를 푼다.
fn quoted_arguments(command: &str) -> Vec<String> {
    let characters: Vec<char> = command.chars().collect();
    let mut arguments = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        if characters[index] == '"' {
            let mut value = String::new();
            let mut cursor = index + 1;
            let mut closed = false;
            while cursor < characters.len() {
                if characters[cursor] == '\\' && cursor + 1 < characters.len() {
                    value.push(characters[cursor]);
                    value.push(characters[cursor + 1]);
                    cursor += 2;
                } else if characters[cursor] == '"' {
                    closed = true;
                    break;
                } else {
                    value.push(characters[cursor]);
                    cursor += 1;
                }
            }
            if closed {
                arguments.push(unescape_argument(&value));
                index = cursor + 1;
            } else {
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    arguments
}

fn unescape_argument(value: &str) -> String {
    let bytes: Vec<char> = value.chars().collect();
    let drive = bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == ':' && matches!(bytes[2], '\\' | '/');
    let unc = value.starts_with("\\\\");
    if drive || unc {
        value.to_string()
    } else {
        let mut text = String::new();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == '\\' && index + 1 < bytes.len() && matches!(bytes[index + 1], '"' | '\\' | '$' | '`') {
                text.push(bytes[index + 1]);
                index += 2;
            } else {
                text.push(bytes[index]);
                index += 1;
            }
        }
        text
    }
}

/// 상태 줄 명령이 Paddock 상태 줄이면 그 명령이 쓰는 파일 경로를, 아니면 None을 돌려준다.
///
/// 설정 폴더가 다른 Paddock(시험용 설정·다른 설치본)이 등록한 명령도 Paddock 상태 줄로 알아본다.
///
/// Examples
/// --------
/// | 명령                                                                                       | 결과                                  |
/// |--------------------------------------------------------------------------------------------|---------------------------------------|
/// | `"/usr/bin/node" "/q/usage/claude-statusline.cjs" "/q/usage/claude.json" "/q/p.json"`       | 스크립트·사용량 파일·이전 파일 경로   |
/// | `npx ccstatusline`                                                                         | None                                  |
pub fn paddock_status_line_files(command: &str) -> Option<StatusLineFiles> {
    let quoted = quoted_arguments(command);
    let index = quoted
        .iter()
        .position(|part| part.ends_with("/claude-statusline.cjs") || part.ends_with("\\claude-statusline.cjs"));
    index.map(|index| StatusLineFiles {
        script: quoted[index].clone(),
        usage_file: quoted.get(index + 1).cloned().unwrap_or_default(),
        previous_file: quoted.get(index + 2).cloned().unwrap_or_default(),
    })
}

/// 상태 줄 명령 인자를 큰따옴표로 감싼다. POSIX는 홈·설치 폴더 이름의 셸 문법이 풀리지 않게 `"`·`\`·`$`·`` ` ``를 이스케이프한다.
pub fn quote_status_line_argument(
    value: &str,
    is_windows: bool,
) -> String {
    let escaped = if is_windows {
        value.to_string()
    } else {
        value.chars().fold(String::new(), |mut text, character| {
            if matches!(character, '"' | '\\' | '$' | '`') {
                text.push('\\');
            }
            text.push(character);
            text
        })
    };
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Codex 세션 기록 한 줄(실제 형식에서 필요한 부분만 남김).
    fn codex_line() -> String {
        json!({"timestamp": "2026-09-24T10:31:36Z", "type": "event_msg", "payload": {"type": "token_count",
            "rate_limits": {"limit_id": "codex",
                "primary": {"used_percent": 41.0, "window_minutes": 10080, "resets_at": 1790846316},
                "secondary": {"used_percent": 12.4, "window_minutes": 300, "resets_at": 1790800000},
                "plan_type": "pro"}}})
        .to_string()
    }

    fn window(
        label: &str,
        used: i64,
        reset: Reset,
    ) -> UsageWindow {
        UsageWindow { label: label.to_string(), used, reset }
    }

    #[test]
    fn test_c_usage_u1_1_the_last_codex_limit_in_a_record_is_read_per_window() {
        let text = [
            "{\"type\":\"other\"}".to_string(),
            codex_line().replace("41.0", "30.0"),
            "not json".to_string(),
            codex_line(),
            String::new(),
        ]
        .join("\n");
        assert_eq!(
            codex_windows(&text),
            vec![window("5h", 12, Reset::At(1790800000)), window("week", 41, Reset::At(1790846316))]
        );
    }

    #[test]
    fn test_c_usage_u1_4_a_null_reset_time_in_a_codex_record_is_an_already_reset_window_and_a_missing_one_is_unknown() {
        let line = |resets: &str| {
            format!(
                r#"{{"payload":{{"rate_limits":{{"limit_id":"codex","primary":{{"used_percent":5,"window_minutes":300{resets}}}}}}}}}"#
            )
        };
        assert_eq!(codex_windows(&line(r#","resets_at":null"#))[0].reset, Reset::Done);
        assert_eq!(codex_windows(&line(""))[0].reset, Reset::Unknown);
        assert_eq!(codex_windows(&line(r#","resets_at":1790800000"#))[0].reset, Reset::At(1790800000));
        assert!(describe(&codex_windows(&line(r#","resets_at":null"#))[0], 200).contains("a new window starts"));
    }

    #[test]
    fn test_c_usage_u1_2_a_record_without_limits_or_with_broken_lines_gives_an_empty_list() {
        assert_eq!(codex_windows("{\"type\":\"session_meta\"}\n{broken"), Vec::new());
        assert_eq!(codex_windows(""), Vec::new());
    }

    #[test]
    fn test_c_usage_u2_1_claude_status_line_input_gives_the_five_hour_and_weekly_windows() {
        let input = json!({"rate_limits": {"five_hour": {"used_percentage": 23.5, "resets_at": 1738425600},
            "seven_day": {"used_percentage": 41.2, "resets_at": 1738857600}}});
        assert_eq!(
            claude_windows(&input),
            vec![window("5h", 24, Reset::At(1738425600)), window("week", 41, Reset::At(1738857600))]
        );
        assert_eq!(claude_windows(&json!({"model": {}})), Vec::new());
    }

    #[test]
    fn test_c_usage_u3_1_a_window_past_its_reset_shows_zero_and_says_a_new_window_starts_with_the_next_request() {
        let windows = [window("5h", 90, Reset::At(100)), window("week", 10, Reset::At(300))];
        let current = current_windows(&windows, 200);
        assert_eq!(current, vec![window("5h", 0, Reset::Done), window("week", 10, Reset::At(300))]);
        assert_eq!(
            describe(&current[0], 200),
            "0% used \u{b7} 100% left \u{b7} reset \u{2014} a new window starts with your next request"
        );
        assert_eq!(describe(&window("5h", 5, Reset::Unknown), 200), "5% used \u{b7} 95% left");
    }

    #[test]
    fn test_c_usage_u3_2_the_tooltip_says_used_left_and_the_time_until_reset() {
        assert_eq!(
            describe(&window("5h", 23, Reset::At(10000 + 2 * 3600 + 600)), 10000),
            "23% used \u{b7} 77% left \u{b7} resets in 2h 10m"
        );
        assert_eq!(
            describe(&window("week", 41, Reset::At(10000 + 3 * 86400 + 3600)), 10000),
            "41% used \u{b7} 59% left \u{b7} resets in 3d 1h"
        );
    }

    #[test]
    fn test_c_usage_u3_3_window_lengths_are_short_labels_and_spelled_out_in_the_tooltip() {
        assert_eq!(
            [300, 10080, 1440, 45].map(window_label),
            ["5h".to_string(), "week".to_string(), "1d".to_string(), "45m".to_string()]
        );
        assert_eq!(
            ["5h", "week", "45m"].map(window_name),
            ["5-hour".to_string(), "weekly".to_string(), "45m".to_string()]
        );
    }

    #[test]
    fn test_c_usage_u4_2_another_paddocks_status_line_command_is_recognised_with_its_file_paths() {
        let other = "\"/usr/bin/node\" \"/tmp/qa/usage/claude-statusline.cjs\" \"/tmp/qa/usage/claude.json\" \"/tmp/qa/usage/previous-statusline.json\"";
        assert_eq!(
            paddock_status_line_files(other),
            Some(StatusLineFiles {
                script: "/tmp/qa/usage/claude-statusline.cjs".to_string(),
                usage_file: "/tmp/qa/usage/claude.json".to_string(),
                previous_file: "/tmp/qa/usage/previous-statusline.json".to_string(),
            })
        );
        assert_eq!(paddock_status_line_files("npx ccstatusline"), None);
    }

    #[test]
    fn test_c_usage_u1_3_codex_records_use_the_default_bucket_and_skip_later_empty_or_other_buckets() {
        let premium = json!({"payload": {"type": "token_count", "rate_limits": {"limit_id": "premium", "primary": null, "secondary": null, "plan_type": "plus"}}}).to_string();
        let other = json!({"payload": {"type": "token_count", "rate_limits": {"limit_id": "other", "primary": {"used_percent": 99, "window_minutes": 300, "resets_at": 1}}}}).to_string();
        let empty = json!({"payload": {"type": "token_count", "rate_limits": {"limit_id": "codex", "primary": null, "secondary": null}}}).to_string();
        let text = [codex_line(), other, empty, premium, String::new()].join("\n");
        let summary: Vec<(String, i64)> =
            codex_windows(&text).into_iter().map(|window| (window.label, window.used)).collect();
        assert_eq!(summary, vec![("5h".to_string(), 12), ("week".to_string(), 41)]);
        let legacy = codex_line().replace("\"limit_id\":\"codex\",", "");
        assert_eq!(codex_windows(&legacy).len(), 2, "records written before limit_id existed are the default bucket");
    }

    #[test]
    fn test_c_usage_u3_4_only_a_window_that_has_not_reset_makes_a_record_current_and_ages_use_one_unit() {
        assert!(has_current_window(&[window("week", 29, Reset::At(300))], 200));
        assert!(!has_current_window(&[window("week", 29, Reset::At(100))], 200));
        assert!(
            !has_current_window(&[window("week", 29, Reset::Unknown)], 200),
            "an unknown reset time does not prove the record is current"
        );
        assert!(!has_current_window(&[], 200));
        assert_eq!([59, 600, 67680, 259200].map(short_age), ["0m", "10m", "18h", "3d"].map(str::to_string));
    }

    #[test]
    fn test_c_usage_u1_x_the_reset_clock_is_the_time_today_the_date_and_time_on_another_day_and_empty_when_unknown() {
        let at = |day: u32, hour: u32, minute: u32| {
            Local.with_ymd_and_hms(2026, 10, day, hour, minute, 0).unwrap().timestamp()
        };
        let now = at(13, 10, 0);
        assert_eq!(reset_clock(&window("5h", 1, Reset::At(at(13, 18, 30))), now), "18:30");
        assert_eq!(reset_clock(&window("5h", 1, Reset::At(at(15, 9, 5))), now), "Oct 15, 09:05");
        assert_eq!(reset_clock(&window("5h", 1, Reset::Done), now), "");
        assert_eq!(reset_clock(&window("5h", 1, Reset::Unknown), now), "");
    }

    #[test]
    fn test_status_line_arguments_are_quoted_for_the_shell_of_each_system() {
        assert_eq!(quote_status_line_argument("/home/a b/$x", false), "\"/home/a b/\\$x\"");
        assert_eq!(quote_status_line_argument("C:\\Program Files\\n", true), "\"C:\\Program Files\\n\"");
        let quoted = quote_status_line_argument("/h/\"q\"/claude-statusline.cjs", false);
        let found = paddock_status_line_files(&quoted).unwrap();
        assert_eq!(found.script, "/h/\"q\"/claude-statusline.cjs", "a quoted path reads back as the same path");
    }
}
