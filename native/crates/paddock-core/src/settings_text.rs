//! 설정 파일 글(주석·끝 쉼표가 섞인 JSON)을 읽는 규칙: 두 설정 파일의 값 합치기, 설정 키 이름, 화면 모양·기본 셸 같은 값 해석.
//!
//! 앱과 데몬이 같은 규칙으로 읽는다. 네이티브 설정 파일에 값이 있으면 그 값이, 없으면 기존 앱 설정 파일의 값이 기본값이다.
//! 사용자가 손으로 고치는 파일이라 읽을 때 주석과 끝 쉼표를 허용하고, 파일이 없거나 깨져도 읽기는 실패하지 않는다.
use serde_json::{Map, Value};

use crate::theme::Mode;

/// 설정 키 이름. 기존 앱과 같은 이름이다.
pub mod key {
    /// 앱을 다시 열 때 지난 터미널을 되살릴지(`always`·`never`).
    pub const RESTORE_ON_RESTART: &str = "paddock.terminal.restoreOnRestart";
    /// 창을 닫을 때 열린 터미널을 어떻게 할지(`ask`·`keep`·`stop`).
    pub const ON_QUIT: &str = "paddock.backend.onQuit";
    /// 화면 배율 단계(배율 = 1.2의 단계 거듭제곱).
    pub const ZOOM_LEVEL: &str = "window.zoomLevel";
    /// 색 테마 이름.
    pub const COLOR_THEME: &str = "workbench.colorTheme";
    /// 에이전트가 오래 일하다 끝났을 때 소리를 낼지(기본 켬).
    pub const AGENT_DONE_SOUND: &str = "paddock.agentDoneSound";
    /// 계정 터미널에서 여는 에이전트의 권한 확인 방식(`ask`·`allowSkip`·`skip`).
    pub const AGENT_PERMISSIONS: &str = "paddock.agents.permissions";
    /// 창 틀(사이드바·탭·상태 줄) 글꼴. 비어 있으면 시스템 기본이다.
    pub const INTERFACE_FONT_FAMILY: &str = "paddock.interfaceFontFamily";
    /// 터미널 글꼴 목록(CSS 글꼴 목록 글자).
    pub const TERMINAL_FONT_FAMILY: &str = "terminal.integrated.fontFamily";
    pub const TERMINAL_FONT_SIZE: &str = "terminal.integrated.fontSize";
    /// 운영체제별 기본 셸 프로필 이름.
    pub const DEFAULT_PROFILE_WINDOWS: &str = "terminal.integrated.defaultProfile.windows";
    pub const DEFAULT_PROFILE_LINUX: &str = "terminal.integrated.defaultProfile.linux";
    pub const DEFAULT_PROFILE_MACOS: &str = "terminal.integrated.defaultProfile.osx";
    /// 파일 목록에서 숨길 이름 패턴(패턴 → 켬/끔).
    pub const FILES_EXCLUDE: &str = "files.exclude";
}

/// 빠른 설정이 받아들이는 값의 범위.
pub mod limit {
    pub const FONT_SIZE_MIN: u32 = 10;
    pub const FONT_SIZE_MAX: u32 = 24;
    /// 터미널 글꼴 크기 기본값(설정이 없거나 숫자가 아닐 때).
    pub const FONT_SIZE_DEFAULT: u32 = 14;
    pub const ZOOM_MIN: f64 = -3.0;
    pub const ZOOM_MAX: f64 = 4.0;
}

/// `files.exclude` 설정이 없을 때 숨기는 패턴. 기존 앱 번들 설정의 기본값과 같다.
const DEFAULT_FILES_EXCLUDE: [&str; 7] =
    ["**/.git", "**/.svn", "**/.hg", "**/CVS", "**/.DS_Store", "**/Thumbs.db", "**/__pycache__"];

/// 터미널 글꼴로 쓸 수 있는 글꼴 이름 뒤에 붙여 쓰는 한글·기본 글꼴 목록(기존 앱이 쓰는 값과 같다).
const TERMINAL_FONT_FALLBACKS: &str = "'Malgun Gothic', 'Apple SD Gothic Neo', 'Noto Sans Mono CJK KR', monospace";

/// 글꼴 목록에서 특정 글꼴이 아닌 CSS 일반 이름.
const GENERIC_FAMILIES: [&str; 8] =
    ["monospace", "sans-serif", "serif", "cursive", "fantasy", "system-ui", "ui-monospace", "ui-sans-serif"];

/// 이 운영체제의 기본 셸 프로필 설정 키.
pub fn default_profile_key() -> &'static str {
    if cfg!(windows) {
        key::DEFAULT_PROFILE_WINDOWS
    } else if cfg!(target_os = "macos") {
        key::DEFAULT_PROFILE_MACOS
    } else {
        key::DEFAULT_PROFILE_LINUX
    }
}

/// 기본 셸 프로필 이름이 가리키는 셸.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileShell {
    /// 데몬에 넘기는 셸 이름(`powershell`·`cmd`·`gitbash`·`wsl`·`bash`).
    pub shell: &'static str,
    /// `shell`이 `wsl`일 때의 배포판 이름. None이면 이 컴퓨터의 기본 배포판이다.
    pub wsl_distribution: Option<String>,
}

/// 기존 앱의 기본 셸 프로필 id를 네이티브 셸 이름으로 바꾼다. 모르는 id면 None이다(호출자가 기본 셸을 쓴다).
///
/// id는 기존 앱 설정의 프로필 이름 그대로 대소문자까지 맞아야 한다. `WSL`은 이 컴퓨터의 기본 배포판(배포판 미지정)이고,
/// `WSL:<배포판>`은 기본이 아닌 배포판을 가리킨다.
///
/// Examples
/// --------
/// | 프로필 id          | 결과                          |
/// |--------------------|-------------------------------|
/// | `PowerShell`       | `powershell`                  |
/// | `Command Prompt`   | `cmd`                         |
/// | `Git Bash`         | `gitbash`                     |
/// | `WSL`              | `wsl`, 배포판 미지정          |
/// | `WSL:Debian`       | `wsl`, 배포판 `Debian`        |
/// | `bash`·`SHELL`     | `bash`                        |
/// | `fish`·`WSL:`·빈 글자 | None                       |
pub fn shell_of_profile(name: &str) -> Option<ProfileShell> {
    let name = name.trim();
    let plain = |shell: &'static str| Some(ProfileShell { shell, wsl_distribution: None });
    match name {
        "PowerShell" => plain("powershell"),
        "Command Prompt" => plain("cmd"),
        "Git Bash" => plain("gitbash"),
        "bash" | "SHELL" => plain("bash"),
        "WSL" => plain("wsl"),
        _ => name
            .strip_prefix("WSL:")
            .map(str::trim)
            .filter(|distribution| !distribution.is_empty())
            .map(|distribution| ProfileShell { shell: "wsl", wsl_distribution: Some(distribution.to_string()) }),
    }
}

/// 네이티브 셸 이름을 기존 앱의 기본 셸 프로필 id로 바꾼다(`shell_of_profile`의 반대).
///
/// 기존 앱은 기본 배포판을 `WSL`로, 그 밖의 배포판을 `WSL:<배포판>`으로 부르므로 이 컴퓨터의 기본 배포판
/// (`default_distribution`)을 받아 구분한다.
///
/// Examples
/// --------
/// | 셸              | 배포판   | 기본 배포판 | 결과             |
/// |-----------------|----------|-------------|------------------|
/// | `powershell`    | -        | -           | `PowerShell`     |
/// | `gitbash`       | -        | -           | `Git Bash`       |
/// | `wsl`           | `Ubuntu` | `Ubuntu`    | `WSL`            |
/// | `wsl`           | `Debian` | `Ubuntu`    | `WSL:Debian`     |
/// | `bash`          | -        | -           | `bash`           |
pub fn profile_of_shell(
    shell: &str,
    wsl_distribution: Option<&str>,
    default_distribution: Option<&str>,
) -> String {
    match (shell, wsl_distribution) {
        ("powershell", _) => "PowerShell".to_string(),
        ("cmd", _) => "Command Prompt".to_string(),
        ("gitbash", _) => "Git Bash".to_string(),
        ("wsl", Some(distribution)) if Some(distribution) != default_distribution => format!("WSL:{distribution}"),
        ("wsl", _) => "WSL".to_string(),
        (other, _) => other.to_string(),
    }
}

/// CSS 글꼴 목록 글자(`'Consolas', 'Malgun Gothic', monospace`)에서 첫 번째 글꼴 이름. 일반 이름(`monospace` 등)뿐이면 None이다.
///
/// Examples
/// --------
/// | 입력                                 | 결과          |
/// |--------------------------------------|---------------|
/// | `'Consolas', 'Malgun Gothic', monospace` | `Consolas` |
/// | `DejaVu Sans Mono, monospace`        | `DejaVu Sans Mono` |
/// | `monospace`                          | None          |
pub fn first_font_family(list: &str) -> Option<String> {
    list.split(',')
        .map(|part| part.trim().trim_matches(|c| c == '\'' || c == '"').trim())
        .find(|name| !name.is_empty() && !GENERIC_FAMILIES.contains(&name.to_lowercase().as_str()))
        .map(str::to_string)
}

/// 터미널 글꼴 설정에 쓰는 글꼴 목록 글자: 고른 글꼴 뒤에 한글·기본 글꼴을 붙인다.
pub fn terminal_font_setting(family: &str) -> String {
    format!("'{family}', {TERMINAL_FONT_FALLBACKS}")
}

/// 터미널 글꼴 크기를 `delta`만큼 바꾼 값. 범위 밖으로 나가지 않는다.
pub fn step_font_size(
    size: u32,
    delta: i32,
) -> u32 {
    size.saturating_add_signed(delta).clamp(limit::FONT_SIZE_MIN, limit::FONT_SIZE_MAX)
}

/// 화면 배율 단계를 `delta`만큼 바꾼 값. 소수 단계는 먼저 반올림하고 범위 밖으로 나가지 않는다.
pub fn step_zoom_level(
    level: f64,
    delta: i32,
) -> f64 {
    (level.round() + f64::from(delta)).clamp(limit::ZOOM_MIN, limit::ZOOM_MAX)
}

/// 창 모양에 관한 설정 값 묶음.
#[derive(Clone, Debug, PartialEq)]
pub struct Appearance {
    pub mode: Mode,
    pub zoom_level: f64,
    pub terminal_font_size: u32,
    /// 터미널 주 글꼴. 없으면 플랫폼 기본이다.
    pub terminal_font_family: Option<String>,
    /// 창 틀 글꼴. 없으면 플랫폼 기본이다.
    pub interface_font_family: Option<String>,
}

/// 다시 열 때 지난 터미널을 되살릴지.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreOnRestart {
    Always,
    Never,
}

/// 주석과 끝 쉼표를 지운다. 문자열 안의 `//`(예: URL)와 `,}`는 그대로 둔다.
///
/// Examples
/// --------
/// | 입력                        | 결과                    |
/// |-----------------------------|-------------------------|
/// | `{"a": 1, // 설명` + 줄바꿈 + `}` | 주석과 끝 쉼표가 빠진 JSON |
/// | `{"u": "http://x",}`        | `{"u": "http://x"}`     |
pub fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(escaped) = next {
                    out.push(escaped);
                    i += 1;
                }
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            out.push('\n');
        } else if c == '/' && next == Some('*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 1;
        } else {
            out.push(c);
        }
        i += 1;
    }
    remove_trailing_commas(&out)
}

/// 닫는 괄호 앞의 쉼표를 지운다(문자열 밖에서만).
fn remove_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    for (i, c) in chars.iter().enumerate() {
        let mut keep = true;
        if in_string {
            if *c == '"' && chars[..i].iter().rev().take_while(|b| **b == '\\').count() % 2 == 0 {
                in_string = false;
            }
        } else if *c == '"' {
            in_string = true;
        } else if *c == ',' {
            let following = chars[i + 1..].iter().find(|next| !next.is_whitespace());
            keep = !matches!(following, Some('}') | Some(']'));
        }
        if keep {
            out.push(*c);
        }
    }
    out
}

/// 설정 파일 내용을 키-값 모음으로 읽는다. 객체가 아니거나 깨졌으면 빈 모음이다.
pub fn parse_settings(text: &str) -> Map<String, Value> {
    try_parse_settings(text).unwrap_or_default()
}

/// 설정 파일 내용을 키-값 모음으로 읽는다. 깨졌거나 객체가 아니면 None이고, 비어 있는 글자는 빈 모음이다.
/// 파일에 다시 쓰기 전에 깨진 파일의 다른 설정을 잃지 않게 구분하려고 쓴다.
///
/// Examples
/// --------
/// | 내용                         | 결과               |
/// |------------------------------|--------------------|
/// | (빈 글자)·공백               | 빈 모음            |
/// | `{ "a": 1, // 메모 }`        | `a` 하나           |
/// | `{ "a": 1, oops`·`[1]`       | None               |
pub fn try_parse_settings(text: &str) -> Option<Map<String, Value>> {
    let stripped = strip_jsonc(text);
    if stripped.trim().is_empty() {
        Some(Map::new())
    } else {
        match serde_json::from_str::<Value>(&stripped) {
            Ok(Value::Object(map)) => Some(map),
            _ => None,
        }
    }
}

/// 두 설정 파일의 내용.
#[derive(Debug, Default)]
pub struct SettingValues {
    native: Map<String, Value>,
    legacy: Map<String, Value>,
}

impl SettingValues {
    /// 파일 내용 두 개(네이티브, 기존 앱)에서 만든다. 파일이 없으면 빈 글자를 준다.
    pub fn from_texts(
        native: &str,
        legacy: &str,
    ) -> Self {
        Self { native: parse_settings(native), legacy: parse_settings(legacy) }
    }

    /// 키의 값. 네이티브 파일에 있으면 그 값이고, 없으면 기존 앱 파일의 값이다.
    pub fn get(
        &self,
        key: &str,
    ) -> Option<&Value> {
        self.native.get(key).or_else(|| self.legacy.get(key))
    }

    /// 화면 배율 단계. 설정이 없거나 숫자가 아니면 0이다.
    pub fn zoom_level(&self) -> f64 {
        self.get(key::ZOOM_LEVEL).and_then(Value::as_f64).unwrap_or(0.0)
    }

    /// 색 테마 이름.
    pub fn color_theme(&self) -> Option<&str> {
        self.get(key::COLOR_THEME).and_then(Value::as_str)
    }

    /// 에이전트 완료 소리를 낼지. 설정이 없거나 불리언이 아니면 켠다.
    pub fn agent_done_sound(&self) -> bool {
        self.get(key::AGENT_DONE_SOUND).and_then(Value::as_bool).unwrap_or(true)
    }

    /// 터미널 글꼴 크기. 범위(10~24) 밖은 가까운 끝으로 맞추고, 설정이 없거나 숫자가 아니면 14다.
    pub fn terminal_font_size(&self) -> u32 {
        self.get(key::TERMINAL_FONT_SIZE)
            .and_then(Value::as_f64)
            .map_or(limit::FONT_SIZE_DEFAULT, |size| size.round() as u32)
            .clamp(limit::FONT_SIZE_MIN, limit::FONT_SIZE_MAX)
    }

    /// 이 운영체제의 기본 셸 프로필 이름. 없거나 문자열이 아니면 None이다.
    pub fn default_profile(&self) -> Option<&str> {
        self.get(default_profile_key()).and_then(Value::as_str)
    }

    /// 파일 목록 숨김 패턴과 켬/끔. 기본 일곱 패턴 위에 설정 값을 덮어쓴다(설정에서 `false`로 끄면 그 패턴만 빠진다).
    /// 값이 불리언이 아닌 항목은 건너뛴다.
    pub fn files_exclude(&self) -> Vec<(String, bool)> {
        let mut patterns: Vec<(String, bool)> =
            DEFAULT_FILES_EXCLUDE.iter().map(|pattern| (pattern.to_string(), true)).collect();
        if let Some(Value::Object(configured)) = self.get(key::FILES_EXCLUDE) {
            for (pattern, on) in configured.iter().filter_map(|(pattern, on)| Some((pattern, on.as_bool()?))) {
                match patterns.iter_mut().find(|(existing, _)| existing == pattern) {
                    Some(existing) => existing.1 = on,
                    None => patterns.push((pattern.clone(), on)),
                }
            }
        }
        patterns
    }

    /// 화면 모양 설정(테마·배율·글꼴)을 한 번에 읽는다.
    pub fn appearance(&self) -> Appearance {
        Appearance {
            mode: Mode::from_theme_name(self.color_theme()),
            zoom_level: self.zoom_level(),
            terminal_font_size: self.terminal_font_size(),
            terminal_font_family: self
                .get(key::TERMINAL_FONT_FAMILY)
                .and_then(Value::as_str)
                .and_then(first_font_family),
            interface_font_family: self
                .get(key::INTERFACE_FONT_FAMILY)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string),
        }
    }

    /// 되살리기 설정. `never`만 끄고, 키가 없거나 알 수 없는 값이거나 주석 처리된 값은 `always`다.
    ///
    /// Examples
    /// --------
    /// | 설정 파일 내용                                      | 결과     |
    /// |-----------------------------------------------------|----------|
    /// | `{ "paddock.terminal.restoreOnRestart": "never" }`  | `Never`  |
    /// | `{ "paddock.terminal.restoreOnRestart": "x" }` · `{}` · 빈 내용 | `Always` |
    /// | `// "paddock.terminal.restoreOnRestart": "never"`   | `Always` |
    pub fn restore_on_restart(&self) -> RestoreOnRestart {
        match self.get(key::RESTORE_ON_RESTART).and_then(Value::as_str) {
            Some("never") => RestoreOnRestart::Never,
            _ => RestoreOnRestart::Always,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Mode;

    #[test]
    fn test_c_d23_jsonc_comments_and_trailing_commas_are_accepted_but_urls_survive() {
        let text = "{\n  // zoom\n  \"window.zoomLevel\": 3, /* big */\n  \"u\": \"http://x//y,}\",\n}";
        let values = SettingValues::from_texts(text, "");
        assert_eq!(values.get("window.zoomLevel").and_then(Value::as_f64), Some(3.0));
        assert_eq!(values.get("u").and_then(Value::as_str), Some("http://x//y,}"));
    }

    #[test]
    fn test_c_d23_native_value_wins_over_the_old_apps_and_broken_files_read_as_empty() {
        let values = SettingValues::from_texts(r#"{"a": 1}"#, r#"{"a": 2, "b": 3}"#);
        assert_eq!(values.get("a").and_then(Value::as_i64), Some(1));
        assert_eq!(values.get("b").and_then(Value::as_i64), Some(3), "the old value is the default");
        let neither = SettingValues::from_texts("{oops", "[1]");
        assert_eq!(neither.get("a"), None);
    }

    #[test]
    fn test_c_restore_pref_only_never_turns_restoring_off() {
        let read = |text: &str| SettingValues::from_texts(text, "").restore_on_restart();
        assert_eq!(read(r#"{ "paddock.terminal.restoreOnRestart": "never" }"#), RestoreOnRestart::Never);
        assert_eq!(read(r#"{ "paddock.terminal.restoreOnRestart": "always" }"#), RestoreOnRestart::Always);
        assert_eq!(read(r#"{ "paddock.terminal.restoreOnRestart": "x" }"#), RestoreOnRestart::Always);
        assert_eq!(read("{}"), RestoreOnRestart::Always);
        assert_eq!(read(""), RestoreOnRestart::Always);
        assert_eq!(read("{\n // \"paddock.terminal.restoreOnRestart\": \"never\"\n}"), RestoreOnRestart::Always);
    }

    #[test]
    fn test_c_restore_pref_native_file_wins_and_the_old_apps_never_is_the_default() {
        let never = r#"{ "paddock.terminal.restoreOnRestart": "never" }"#;
        let always = r#"{ "paddock.terminal.restoreOnRestart": "always" }"#;
        assert_eq!(SettingValues::from_texts("", never).restore_on_restart(), RestoreOnRestart::Never);
        assert_eq!(SettingValues::from_texts(always, never).restore_on_restart(), RestoreOnRestart::Always);
        assert_eq!(SettingValues::from_texts(never, always).restore_on_restart(), RestoreOnRestart::Never);
    }
    #[test]
    fn test_try_parse_settings_tells_broken_files_from_empty_ones() {
        assert_eq!(try_parse_settings("  \n"), Some(Map::new()));
        assert_eq!(
            try_parse_settings("{ \"a\": 1, // note\n}").and_then(|map| map.get("a").cloned()),
            Some(Value::from(1))
        );
        assert_eq!(try_parse_settings("{ \"a\": 1, oops"), None);
        assert_eq!(try_parse_settings("[1]"), None);
    }

    #[test]
    fn test_c_d41_default_shell_profile_name_maps_to_shell_name() {
        // 기존 앱 번들 설정의 Windows 프로필 id 그대로: `WSL`은 기본 배포판, `WSL:<배포판>`은 그 밖의 배포판.
        let shell = |name: &str| shell_of_profile(name).map(|found| (found.shell, found.wsl_distribution));
        assert_eq!(shell("PowerShell"), Some(("powershell", None)));
        assert_eq!(shell("Command Prompt"), Some(("cmd", None)));
        assert_eq!(shell("Git Bash"), Some(("gitbash", None)));
        assert_eq!(shell("WSL"), Some(("wsl", None)));
        assert_eq!(shell("WSL:Ubuntu-24.04"), Some(("wsl", Some("Ubuntu-24.04".to_string()))));
        assert_eq!(shell("bash"), Some(("bash", None)));
        assert_eq!(shell("SHELL"), Some(("bash", None)));
    }

    #[test]
    fn test_c_d41_unknown_default_shell_profile_falls_back_to_default() {
        for name in ["", "  ", "fish", "My Custom Shell", "WSL:", "WSL: ", "Ubuntu (WSL)", "powershell"] {
            assert_eq!(shell_of_profile(name), None, "{name:?}");
        }
    }

    #[test]
    fn test_c_d47_profile_name_written_for_a_shell_reads_back_as_the_same_shell() {
        // 기본 배포판은 `WSL`로 쓰고 읽으면 배포판 미지정(기본 배포판)이다. 그 밖의 배포판은 `WSL:<배포판>`이다.
        let written = |shell: &str, distribution: Option<&str>| profile_of_shell(shell, distribution, Some("Ubuntu"));
        assert_eq!(written("powershell", None), "PowerShell");
        assert_eq!(written("cmd", None), "Command Prompt");
        assert_eq!(written("gitbash", None), "Git Bash");
        assert_eq!(written("bash", None), "bash");
        assert_eq!(written("wsl", Some("Ubuntu")), "WSL");
        assert_eq!(written("wsl", Some("Debian")), "WSL:Debian");
        assert_eq!(written("wsl", None), "WSL");
        for (shell, distribution) in [("powershell", None), ("cmd", None), ("gitbash", None), ("bash", None)] {
            let found = shell_of_profile(&written(shell, distribution)).expect(shell);
            assert_eq!((found.shell, found.wsl_distribution.as_deref()), (shell, distribution));
        }
        let debian = shell_of_profile(&written("wsl", Some("Debian"))).expect("WSL:Debian");
        assert_eq!((debian.shell, debian.wsl_distribution.as_deref()), ("wsl", Some("Debian")));
        let default = shell_of_profile(&written("wsl", Some("Ubuntu"))).expect("WSL");
        assert_eq!((default.shell, default.wsl_distribution), ("wsl", None));
    }

    #[test]
    fn test_c_d41_old_apps_sound_off_is_read_when_native_has_no_value() {
        assert!(SettingValues::from_texts("", "").agent_done_sound());
        assert!(!SettingValues::from_texts("", r#"{"paddock.agentDoneSound": false}"#).agent_done_sound());
        let native_on = r#"{"paddock.agentDoneSound": true}"#;
        assert!(SettingValues::from_texts(native_on, r#"{"paddock.agentDoneSound": false}"#).agent_done_sound());
        assert!(SettingValues::from_texts(r#"{"paddock.agentDoneSound": "no"}"#, "").agent_done_sound());
    }

    #[test]
    fn test_c_d41_appearance_follows_the_old_apps_values_until_native_has_its_own() {
        let old = r#"{
            "window.zoomLevel": 3, "workbench.colorTheme": "Mermaid Light", "terminal.integrated.fontSize": 18,
            "terminal.integrated.fontFamily": "'Cascadia Mono', 'Malgun Gothic', monospace",
            "paddock.interfaceFontFamily": "Inter"
        }"#;
        let appearance = SettingValues::from_texts("", old).appearance();
        assert_eq!(appearance.mode, Mode::Light);
        assert_eq!(appearance.zoom_level, 3.0);
        assert_eq!(appearance.terminal_font_size, 18);
        assert_eq!(appearance.terminal_font_family.as_deref(), Some("Cascadia Mono"));
        assert_eq!(appearance.interface_font_family.as_deref(), Some("Inter"));
        let native = SettingValues::from_texts(r#"{"workbench.colorTheme": "Mermaid Dark"}"#, old).appearance();
        assert_eq!((native.mode, native.zoom_level), (Mode::Dark, 3.0));
        let none = SettingValues::from_texts("", "").appearance();
        assert_eq!((none.mode, none.zoom_level, none.terminal_font_size), (Mode::Dark, 0.0, 14));
        assert_eq!((none.terminal_font_family, none.interface_font_family), (None, None));
    }

    #[test]
    fn test_c_d41_terminal_font_size_is_kept_inside_the_range_and_bad_values_read_as_default() {
        let size = |text: &str| SettingValues::from_texts(text, "").terminal_font_size();
        assert_eq!(size(r#"{"terminal.integrated.fontSize": 99}"#), 24);
        assert_eq!(size(r#"{"terminal.integrated.fontSize": 3}"#), 10);
        assert_eq!(size(r#"{"terminal.integrated.fontSize": 16.4}"#), 16);
        assert_eq!(size(r#"{"terminal.integrated.fontSize": "big"}"#), 14);
    }

    #[test]
    fn test_c_d41_font_family_setting_uses_the_first_named_family_and_skips_generic_names() {
        let first = |value: &str| first_font_family(value);
        assert_eq!(first("'Consolas', 'Malgun Gothic', monospace").as_deref(), Some("Consolas"));
        assert_eq!(first("\"Fira Code\", monospace").as_deref(), Some("Fira Code"));
        assert_eq!(first("DejaVu Sans Mono, monospace").as_deref(), Some("DejaVu Sans Mono"));
        assert_eq!(first("monospace"), None);
        assert_eq!(first("  "), None);
        let written = terminal_font_setting("JetBrains Mono");
        assert!(written.starts_with("'JetBrains Mono', "), "{written}");
        assert!(written.contains("'Malgun Gothic'"), "{written}");
        assert_eq!(first(&written).as_deref(), Some("JetBrains Mono"));
    }

    #[test]
    fn test_c_d41_quick_setting_steps_stop_at_the_limits() {
        assert_eq!(step_font_size(14, 1), 15);
        assert_eq!(step_font_size(limit::FONT_SIZE_MAX, 1), limit::FONT_SIZE_MAX);
        assert_eq!(step_font_size(limit::FONT_SIZE_MIN, -1), limit::FONT_SIZE_MIN);
        assert_eq!(step_zoom_level(0.0, 1), 1.0);
        assert_eq!(step_zoom_level(2.6, 1), 4.0, "a fractional level rounds first");
        assert_eq!(step_zoom_level(limit::ZOOM_MAX, 1), limit::ZOOM_MAX);
        assert_eq!(step_zoom_level(limit::ZOOM_MIN, -1), limit::ZOOM_MIN);
    }
}
