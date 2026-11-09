//! Claude Code가 실행 인자(`--settings <파일>`)로 덧붙여 읽는 Paddock 전용 설정 파일의 내용과, 그 내용을 정하는 마지막 설정(`usage/state.json`).
//!
//! 상태 줄(사용량 표시)과 메모리 안내 훅은 이 컴퓨터의 Paddock 설치 경로를 가리키는 값이다. 사용자의 `settings.json`에 적으면 그 파일을
//! git으로 공유하는 다른 컴퓨터까지 넘어가 깨지므로, 이 컴퓨터의 설정 폴더에 따로 두고 실행할 때만 덧붙인다. 플래그 설정은 사용자 설정보다
//! 앞서므로 상태 줄은 이 파일의 것이 쓰이고, 훅은 사용자 훅과 합쳐진다. 파일 내용은 `state.json`과 설치 경로의 순수한 결과이므로
//! 기존 앱과 같은 규칙으로 쓰면 같은 바이트가 된다(키 순서·들여쓰기·끝 줄바꿈 포함).
use serde_json::{Map, Value, json};

use crate::usage_model::{ClaudeState, quote_status_line_argument};

/// 플래그 설정 파일의 자리. 설정 폴더 아래 사용자 홈의 dotfiles 저장소 밖이다.
pub struct FlagSettings;

impl FlagSettings {
    pub const DIRECTORY: &'static str = "claude";
    pub const FILE: &'static str = "flag-settings.json";
}

/// 메모리 안내 훅의 이름과 기준값.
pub struct MemoryHook;

impl MemoryHook {
    /// 에이전트가 메모리를 쓰는 도구에만 반응한다(`memory-hook.sh`가 같은 목록을 한 번 더 거른다).
    pub const MATCHER: &'static str = "Bash|Agent|Task";
    pub const SCRIPT_NAME: &'static str = "memory-hook.sh";
    /// 스크립트와 같은 폴더에 두면 스크립트가 아무것도 하지 않는다(`memory-hook.sh`가 확인한다).
    pub const OFF_MARKER: &'static str = "memory-hook.off";
    pub const DEFAULT_MINIMUM_MB: u64 = 4096;
}

/// 설정 폴더 기준 플래그 설정 파일 경로. `separator`는 그 경로를 읽을 환경의 구분자다(WSL 안은 `/`).
///
/// Examples
/// --------
/// | config_dir                | separator | 결과                                               |
/// |---------------------------|-----------|----------------------------------------------------|
/// | `/home/me/.paddock/config`| `/`       | `/home/me/.paddock/config/claude/flag-settings.json` |
pub fn flag_settings_path(
    config_dir: &str,
    separator: char,
) -> String {
    let base = config_dir.trim_end_matches(separator);
    format!("{base}{separator}{}{separator}{}", FlagSettings::DIRECTORY, FlagSettings::FILE)
}

/// 값을 POSIX 셸 한 단어로 감싼다. 작은따옴표는 닫고 이스케이프한 뒤 다시 연다.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// 기준값 입력을 정수 MB로 바꾼다. 유한한 0 이상의 수만 받고(소수는 내림), 그 밖에는 기본 4096이다.
///
/// Examples
/// --------
/// | 입력      | 결과 |
/// |-----------|------|
/// | 2048      | 2048 |
/// | 1.9       | 1    |
/// | -5 · NaN  | 4096 |
pub fn normalize_minimum_mb(minimum_mb: f64) -> u64 {
    if minimum_mb.is_finite() && minimum_mb >= 0.0 { minimum_mb.floor() as u64 } else { MemoryHook::DEFAULT_MINIMUM_MB }
}

/// 훅 명령 문자열. 기준값은 환경 변수로 넘겨 스크립트 파일을 바꾸지 않고도 설정으로 달라진다.
///
/// Examples
/// --------
/// | script                      | minimum_mb | 결과                                                         |
/// |-----------------------------|------------|--------------------------------------------------------------|
/// | `/home/me/u/memory-hook.sh` | 4096       | `PADDOCK_MEMORY_MIN_MB=4096 sh '/home/me/u/memory-hook.sh'`  |
pub fn memory_hook_command(
    script: &str,
    minimum_mb: f64,
) -> String {
    format!("PADDOCK_MEMORY_MIN_MB={} sh {}", normalize_minimum_mb(minimum_mb), shell_quote(script))
}

/// 이 모듈이 넣은 훅 명령인지. 명령에 `memory-hook.sh`가 들어 있으면 그렇다고 본다(사용자가 직접 만든 훅과 구분하는 기준).
pub fn is_memory_hook_command(command: &Value) -> bool {
    command.as_str().is_some_and(|text| text.contains(MemoryHook::SCRIPT_NAME))
}

/// PreToolUse 항목 목록에서 이 모듈의 훅만 뺀 새 목록. 훅이 비게 된 항목은 버리고, 사용자의 다른 훅은 그대로 둔다.
pub fn without_memory_hook(entries: &[Value]) -> Vec<Value> {
    let mut kept = Vec::new();
    for entry in entries {
        match entry.get("hooks").and_then(Value::as_array) {
            Some(hooks) => {
                let remaining: Vec<Value> = hooks
                    .iter()
                    .filter(|hook| !is_memory_hook_command(hook.get("command").unwrap_or(&Value::Null)))
                    .cloned()
                    .collect();
                if remaining.len() == hooks.len() {
                    kept.push(entry.clone());
                } else if !remaining.is_empty() {
                    let mut changed = entry.clone();
                    changed["hooks"] = Value::Array(remaining);
                    kept.push(changed);
                }
            }
            None => kept.push(entry.clone()),
        }
    }
    kept
}

/// 설정에 훅을 넣은 새 설정. 이미 있으면 같은 명령으로 바꾸므로 여러 번 불러도 한 번만 들어간다. 입력 설정은 바꾸지 않는다.
pub fn install_memory_hook(
    settings: &Value,
    command: &str,
) -> Value {
    let existing: Vec<Value> = settings
        .get("hooks")
        .and_then(|hooks| hooks.get("PreToolUse"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut entries = without_memory_hook(&existing);
    entries.push(json!({ "matcher": MemoryHook::MATCHER, "hooks": [{ "type": "command", "command": command }] }));
    let mut result = settings.clone();
    if !result.is_object() {
        result = Value::Object(Map::new());
    }
    let mut hooks = result.get("hooks").and_then(Value::as_object).cloned().unwrap_or_default();
    hooks.insert("PreToolUse".to_string(), Value::Array(entries));
    result["hooks"] = Value::Object(hooks);
    result
}

/// 설정에서 이 모듈의 훅을 뺀 새 설정. 빼고 나서 비게 된 PreToolUse·hooks는 지워 설정이 원래 모양으로 돌아가게 한다.
/// 넣은 훅이 없으면 받은 설정과 같은 값을 돌려준다.
pub fn remove_memory_hook(settings: &Value) -> Value {
    let mut result = settings.clone();
    if let Some(existing) = settings.get("hooks").and_then(|hooks| hooks.get("PreToolUse")).and_then(Value::as_array) {
        let kept = without_memory_hook(existing);
        if kept != *existing {
            let mut hooks = settings["hooks"].as_object().cloned().unwrap_or_default();
            if kept.is_empty() {
                hooks.shift_remove("PreToolUse");
            } else {
                hooks.insert("PreToolUse".to_string(), Value::Array(kept));
            }
            if hooks.is_empty() {
                result.as_object_mut().map(|map| map.shift_remove("hooks"));
            } else {
                result["hooks"] = Value::Object(hooks);
            }
        }
    }
    result
}

/// 설정에 이 모듈의 훅이 들어 있는지.
pub fn has_memory_hook(settings: &Value) -> bool {
    settings.get("hooks").and_then(|hooks| hooks.get("PreToolUse")).and_then(Value::as_array).is_some_and(|entries| {
        entries.iter().any(|entry| {
            entry.get("hooks").and_then(Value::as_array).is_some_and(|hooks| {
                hooks.iter().any(|hook| is_memory_hook_command(hook.get("command").unwrap_or(&Value::Null)))
            })
        })
    })
}

/// 켜져 있는 Paddock 기능만 담은 플래그 설정. 둘 다 꺼져 있으면 빈 객체다. 키 순서는 `statusLine`, `hooks`다.
///
/// Examples
/// --------
/// | status_line_command | memory_hook_command | 결과                                              |
/// |---------------------|---------------------|---------------------------------------------------|
/// | `node s.cjs u`      | (없음)              | `{ statusLine: { type: "command", command } }`    |
/// | (없음)              | `sh m.sh`           | `{ hooks: { PreToolUse: [메모리 안내 훅] } }`     |
/// | (없음)              | (없음)              | `{}`                                              |
pub fn build_flag_settings(
    status_line_command: Option<&str>,
    memory_hook_command: Option<&str>,
) -> Value {
    let mut settings = Value::Object(Map::new());
    if let Some(command) = status_line_command.filter(|command| !command.is_empty()) {
        settings["statusLine"] = json!({ "type": "command", "command": command });
    }
    if let Some(command) = memory_hook_command.filter(|command| !command.is_empty()) {
        settings = install_memory_hook(&settings, command);
    }
    settings
}

/// 파일에 쓸 글: 2칸 들여쓰기 JSON과 끝 줄바꿈 하나(기존 앱의 `JSON.stringify(value, null, 2) + "\n"`와 같은 바이트).
pub fn render_json(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string());
    text.push('\n');
    text
}

/// 상태 줄 명령 문자열: `node 스크립트 기본사용량파일`. node 실행 파일을 모르면 None이다.
/// 어느 계정의 사용량인지는 스크립트가 실행 환경의 `CLAUDE_CONFIG_DIR`로 정하므로 계정별 경로를 받지 않는다.
///
/// Examples
/// --------
/// | node                        | is_windows | 결과                                                                 |
/// |-----------------------------|------------|----------------------------------------------------------------------|
/// | `C:\Program Files\nodejs\node.exe` | true | `"C:\Program Files\nodejs\node.exe" "C:\u\claude-statusline.cjs" "C:\u\claude.json"` |
/// | `/h/.nvm/bin/node`          | false      | `"/h/.nvm/bin/node" "/h/u/claude-statusline.cjs" "/h/u/claude.json"` |
/// | (없음)                      | -          | None                                                                 |
pub fn status_line_command(
    node: Option<&str>,
    script: &str,
    usage_file: &str,
    is_windows: bool,
) -> Option<String> {
    node.filter(|node| !node.is_empty()).map(|node| {
        [node, script, usage_file]
            .iter()
            .map(|part| quote_status_line_argument(part, is_windows))
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// `where node`(Windows) 또는 `which node`의 출력에서 node 실행 파일 경로: 첫 줄을 다듬은 것. 비면 None이다.
pub fn first_line_path(output: &str) -> Option<String> {
    output.lines().next().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string)
}

/// 메모리 안내의 마지막 설정: 켜짐 여부와 기준값(MB).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemorySetting {
    pub enabled: bool,
    pub minimum_mb: f64,
}

/// `usage/state.json`이 담은 마지막 설정. 이 파일이 플래그 설정 파일의 내용을 정한다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UsageState {
    pub claude: ClaudeState,
    pub memory: Option<MemorySetting>,
}

impl UsageState {
    /// 파일 내용(없거나 깨졌으면 None)에서 읽는다. 모르는 값은 `Unset`·메모리 없음이다.
    pub fn from_value(value: Option<&Value>) -> Self {
        let claude = value
            .and_then(|state| state.get("claude"))
            .and_then(Value::as_str)
            .map_or(ClaudeState::Unset, ClaudeState::parse);
        let memory =
            value.and_then(|state| state.get("memory")).and_then(Value::as_object).map(|memory| MemorySetting {
                enabled: memory.get("enabled").and_then(Value::as_bool) == Some(true),
                minimum_mb: memory.get("minimumMb").and_then(Value::as_f64).unwrap_or(f64::NAN),
            });
        Self { claude, memory }
    }

    /// 메모리 안내가 켜져 있는지.
    pub fn memory_enabled(&self) -> bool {
        self.memory.is_some_and(|memory| memory.enabled)
    }
}

/// 파일 내용에 사용량 표시 값을 합쳐 쓸 새 내용. 다른 키는 그대로 두고 순서도 유지한다(기존 앱의 `{...state, ...changes}`).
pub fn merged_claude_state(
    current: Option<&Value>,
    setting: ClaudeState,
) -> Value {
    let mut state = current.and_then(Value::as_object).cloned().unwrap_or_default();
    let text = if setting == ClaudeState::On { "on" } else { "off" };
    state.insert("claude".to_string(), Value::String(text.to_string()));
    Value::Object(state)
}

/// 파일 내용에 메모리 안내 값을 합쳐 쓸 새 내용. 다른 키는 그대로 두고 순서도 유지한다.
pub fn merged_memory_state(
    current: Option<&Value>,
    enabled: bool,
    minimum_mb: f64,
) -> Value {
    let mut state = current.and_then(Value::as_object).cloned().unwrap_or_default();
    let minimum = if minimum_mb.is_finite() && minimum_mb.fract() == 0.0 && minimum_mb >= 0.0 {
        Value::from(minimum_mb as u64)
    } else {
        Value::from(MemoryHook::DEFAULT_MINIMUM_MB)
    };
    state.insert("memory".to_string(), json!({ "enabled": enabled, "minimumMb": minimum }));
    Value::Object(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = "/home/me/.paddock/config/usage/memory-hook.sh";

    fn user_entry() -> Value {
        json!({ "matcher": "*", "hooks": [{ "type": "command", "command": "python3 ~/.claude/hooks/mem-guard.py" }] })
    }

    fn command() -> String {
        memory_hook_command(SCRIPT, 4096.0)
    }

    #[test]
    fn test_c_memhook_set_1_the_command_passes_the_threshold_by_environment_variable_and_quotes_the_path_as_one_word() {
        assert_eq!(command(), "PADDOCK_MEMORY_MIN_MB=4096 sh '/home/me/.paddock/config/usage/memory-hook.sh'");
        assert_eq!(
            memory_hook_command("/a b/it's/memory-hook.sh", 2048.0),
            "PADDOCK_MEMORY_MIN_MB=2048 sh '/a b/it'\\''s/memory-hook.sh'"
        );
        assert!(memory_hook_command(SCRIPT, f64::NAN).starts_with("PADDOCK_MEMORY_MIN_MB=4096 "));
        assert!(memory_hook_command(SCRIPT, -5.0).starts_with("PADDOCK_MEMORY_MIN_MB=4096 "));
        assert!(memory_hook_command(SCRIPT, 1.9).starts_with("PADDOCK_MEMORY_MIN_MB=1 "));
    }

    #[test]
    fn test_c_memhook_set_2_installing_keeps_the_users_other_hooks_and_settings_and_the_input_unchanged() {
        let settings = json!({ "theme": "dark", "hooks": { "PreToolUse": [user_entry()], "Stop": [{ "hooks": [] }] } });
        let before = settings.clone();
        let next = install_memory_hook(&settings, &command());
        assert_eq!(settings, before);
        assert_eq!(next["theme"], "dark");
        assert_eq!(next["hooks"]["Stop"], settings["hooks"]["Stop"]);
        assert_eq!(next["hooks"]["PreToolUse"][0], user_entry());
        assert_eq!(
            next["hooks"]["PreToolUse"][1],
            json!({ "matcher": "Bash|Agent|Task", "hooks": [{ "type": "command", "command": command() }] })
        );
        assert!(has_memory_hook(&next));
        assert!(!has_memory_hook(&settings));
    }

    #[test]
    fn test_c_memhook_set_3_installing_twice_keeps_one_entry_and_a_new_threshold_replaces_it() {
        let once = install_memory_hook(&json!({}), &command());
        let twice = install_memory_hook(&once, &command());
        assert_eq!(twice["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        let changed = install_memory_hook(&twice, &memory_hook_command(SCRIPT, 8192.0));
        assert_eq!(changed["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert!(
            changed["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .starts_with("PADDOCK_MEMORY_MIN_MB=8192 ")
        );
    }

    #[test]
    fn test_c_memhook_set_4_removing_drops_only_this_modules_hook_and_keeps_the_users_in_the_same_entry() {
        let mixed = json!({ "hooks": { "PreToolUse": [{ "matcher": "*", "hooks": [user_entry()["hooks"][0].clone(), { "type": "command", "command": command() }] }] } });
        let removed = remove_memory_hook(&mixed);
        assert_eq!(
            removed["hooks"]["PreToolUse"],
            json!([{ "matcher": "*", "hooks": [user_entry()["hooks"][0].clone()] }])
        );
        assert!(!has_memory_hook(&removed));
    }

    #[test]
    fn test_c_memhook_set_5_a_settings_with_only_the_hook_returns_to_its_original_shape() {
        let installed = install_memory_hook(&json!({ "theme": "dark" }), &command());
        assert_eq!(remove_memory_hook(&installed), json!({ "theme": "dark" }));
        let with_others = install_memory_hook(&json!({ "hooks": { "Stop": [{ "hooks": [] }] } }), &command());
        assert_eq!(remove_memory_hook(&with_others), json!({ "hooks": { "Stop": [{ "hooks": [] }] } }));
    }

    #[test]
    fn test_c_memhook_set_6_removing_without_a_hook_gives_back_the_same_settings() {
        let settings = json!({ "hooks": { "PreToolUse": [user_entry()] } });
        assert_eq!(remove_memory_hook(&settings), settings);
        assert_eq!(remove_memory_hook(&json!({})), json!({}));
    }

    #[test]
    fn test_c_d73_flag_settings_bytes_match_the_electron_rules_for_the_three_shapes() {
        // JSON.stringify(value, null, 2) + "\n"와 같은 글자: 키 순서는 statusLine, hooks.
        assert_eq!(render_json(&build_flag_settings(None, None)), "{}\n");
        let status = "\"C:\\Program Files\\nodejs\\node.exe\" \"C:\\Users\\me\\.paddock\\config\\usage\\claude-statusline.cjs\" \"C:\\Users\\me\\.paddock\\config\\usage\\claude.json\"";
        assert_eq!(
            render_json(&build_flag_settings(Some(status), None)),
            concat!(
                "{\n  \"statusLine\": {\n    \"type\": \"command\",\n    \"command\": ",
                "\"\\\"C:\\\\Program Files\\\\nodejs\\\\node.exe\\\" \\\"C:\\\\Users\\\\me\\\\.paddock\\\\config\\\\usage\\\\claude-statusline.cjs\\\" \\\"C:\\\\Users\\\\me\\\\.paddock\\\\config\\\\usage\\\\claude.json\\\"\"",
                "\n  }\n}\n"
            )
        );
        let both = render_json(&build_flag_settings(Some("node s u"), Some(&command())));
        let status_at = both.find("statusLine").unwrap();
        let hooks_at = both.find("\"hooks\"").unwrap();
        assert!(status_at < hooks_at, "statusLine comes first: {both}");
        assert!(
            both.find("\"matcher\"").unwrap() < both.rfind("\"hooks\"").unwrap(),
            "matcher comes before the entry's hooks: {both}"
        );
        assert!(both.ends_with("}\n"));
    }

    #[test]
    fn test_c_usage_u4_2_the_status_line_command_quotes_each_part_and_needs_a_node_path() {
        assert_eq!(
            status_line_command(Some("/h/.nvm/bin/node"), "/h/u/claude-statusline.cjs", "/h/u/claude.json", false)
                .as_deref(),
            Some("\"/h/.nvm/bin/node\" \"/h/u/claude-statusline.cjs\" \"/h/u/claude.json\"")
        );
        assert_eq!(
            status_line_command(
                Some("C:\\Program Files\\nodejs\\node.exe"),
                "C:\\u\\claude-statusline.cjs",
                "C:\\u\\claude.json",
                true
            )
            .as_deref(),
            Some("\"C:\\Program Files\\nodejs\\node.exe\" \"C:\\u\\claude-statusline.cjs\" \"C:\\u\\claude.json\"")
        );
        assert_eq!(status_line_command(None, "s", "u", false), None);
        assert_eq!(first_line_path("C:\\a\\node.exe\r\nC:\\b\\node.exe\r\n").as_deref(), Some("C:\\a\\node.exe"));
        assert_eq!(first_line_path("\n"), None);
    }

    #[test]
    fn test_c_d73_the_usage_state_reads_unknown_values_as_unset_and_merges_without_losing_other_keys() {
        let read = |text: &str| UsageState::from_value(serde_json::from_str::<Value>(text).ok().as_ref());
        assert_eq!(read(r#"{"claude":"on"}"#).claude, ClaudeState::On);
        assert!(read(r#"{"claude":"off","memory":{"enabled":true,"minimumMb":2048}}"#).memory_enabled());
        assert_eq!(read("not json").claude, ClaudeState::Unset);
        assert_eq!(read(r#"{"claude":"maybe"}"#).claude, ClaudeState::Unset);
        assert!(!read(r#"{"memory":{"enabled":"yes"}}"#).memory_enabled());
        let current: Value = serde_json::from_str(r#"{"claude":"on","other":1}"#).unwrap();
        let next = merged_memory_state(Some(&current), true, 4608.0);
        assert_eq!(
            render_json(&next),
            "{\n  \"claude\": \"on\",\n  \"other\": 1,\n  \"memory\": {\n    \"enabled\": true,\n    \"minimumMb\": 4608\n  }\n}\n"
        );
        let off = merged_claude_state(Some(&next), ClaudeState::Off);
        assert_eq!(off["claude"], "off");
        assert_eq!(off["memory"]["minimumMb"], 4608);
    }

    #[test]
    fn test_c_d73_the_flag_settings_path_joins_with_the_readers_separator() {
        assert_eq!(
            flag_settings_path("/home/me/.paddock/config", '/'),
            "/home/me/.paddock/config/claude/flag-settings.json"
        );
        assert_eq!(
            flag_settings_path("C:\\Users\\me\\.paddock\\config", '\\'),
            "C:\\Users\\me\\.paddock\\config\\claude\\flag-settings.json"
        );
    }
}
