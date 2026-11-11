//! 앱 설정: 네이티브 설정 파일(`~/.paddock/native/settings.json`)과 기존 앱 설정 파일(`~/.paddock/config/settings.json`).
//!
//! 두 앱이 같은 설정 키를 공유한다. 네이티브 파일에 값이 있으면 그 값이, 없으면 기존 앱 파일의 값이 기본값으로 쓰인다.
//! 기존 앱 파일은 읽기만 한다. 파일 읽기 규칙(주석·끝 쉼표 허용, 두 파일 합치기)은 데몬과 함께 쓰므로 core의 `settings_text`가 쥐고,
//! 이 모듈은 값 꺼내기와 네이티브 파일 쓰기만 맡는다.
use std::io;

use paddock_core::account_launch::Permissions;
use paddock_core::settings_text::{Appearance, SettingValues, try_parse_settings};
use paddock_protocol::daemon_info::{ensure_private_dir, native_dir, write_private_file};
use paddock_protocol::shared_config::{native_settings_path, read_settings};
use serde_json::{Map, Value};

use crate::app_log;

pub use paddock_core::settings_text::key;

/// 두 설정 파일의 내용.
#[derive(Debug, Default)]
pub struct Settings {
    values: SettingValues,
}

impl Settings {
    /// 두 설정 파일을 읽는다.
    pub fn load() -> Self {
        Self { values: read_settings() }
    }

    /// 파일 내용 두 개에서 만든다. 파일을 읽지 않고 해석 규칙만 확인하는 시험이 쓴다.
    #[cfg(test)]
    fn from_texts(
        native: &str,
        legacy: &str,
    ) -> Self {
        Self { values: SettingValues::from_texts(native, legacy) }
    }

    /// 키의 값. 네이티브 파일에 있으면 그 값이고, 없으면 기존 앱 파일의 값이다.
    pub fn get(
        &self,
        key: &str,
    ) -> Option<&Value> {
        self.values.get(key)
    }

    /// 두 파일 값을 합쳐 읽는 규칙을 쥔 값 묶음.
    pub fn values(&self) -> &SettingValues {
        &self.values
    }

    /// 화면 배율 단계. 설정이 없거나 숫자가 아니면 0이다.
    pub fn zoom_level(&self) -> f64 {
        self.values.zoom_level()
    }

    /// 색 테마 이름.
    pub fn color_theme(&self) -> Option<&str> {
        self.values.color_theme()
    }

    /// 에이전트 완료 소리를 낼지. 설정이 없거나 불리언이 아니면 켠다.
    pub fn agent_done_sound(&self) -> bool {
        self.values.agent_done_sound()
    }

    /// 창 모양(테마·배율·글꼴) 설정.
    pub fn appearance(&self) -> Appearance {
        self.values.appearance()
    }

    /// 계정 터미널의 권한 확인 방식. 설정이 없거나 모르는 값이면 CLI 기본대로 묻는다(`ask`).
    pub fn agent_permissions(&self) -> Permissions {
        self.get(key::AGENT_PERMISSIONS)
            .and_then(Value::as_str)
            .and_then(|value| Permissions::parse(value).ok())
            .unwrap_or_default()
    }

    /// 종료 설정 글자(`ask`·`keep`·`stop`). 없거나 문자열이 아니면 None이다.
    pub fn on_quit(&self) -> Option<&str> {
        self.get(key::ON_QUIT).and_then(Value::as_str)
    }
}

/// 네이티브 설정 파일에 키 하나를 쓴다. 파일의 다른 설정은 그대로 두고, 기존 앱 파일은 건드리지 않는다.
///
/// 주석이 있던 파일은 쓰면서 주석이 사라진다(읽을 때만 주석을 허용한다). 파일이 깨져 읽을 수 없으면
/// `settings.json.broken`으로 옮겨 두고 새 파일에 이 키만 쓴다(손으로 고치던 내용을 잃지 않는다).
/// 소유자만 읽는 임시 파일(`settings.json.tmp`)에 다 쓴 뒤 바꿔치워, 쓰는 도중 앱이 죽어도 이전 파일이 남는다.
pub fn write_native(
    key: &str,
    value: Value,
) -> io::Result<()> {
    let path = native_settings_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    ensure_private_dir(&native_dir())?;
    let mut settings = match try_parse_settings(&text) {
        Some(settings) => settings,
        None => {
            std::fs::rename(&path, path.with_extension("json.broken"))?;
            app_log::event("the native settings file was unreadable and was set aside as settings.json.broken");
            Map::new()
        }
    };
    settings.insert(key.to_string(), value);
    let text = serde_json::to_string_pretty(&Value::Object(settings)).expect("settings are plain data");
    write_private_file(&path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempHome;

    #[test]
    fn test_c_d37_agent_done_sound_is_on_unless_a_setting_turns_it_off() {
        assert!(Settings::from_texts("", "").agent_done_sound());
        assert!(!Settings::from_texts("", r#"{"paddock.agentDoneSound": false}"#).agent_done_sound());
        // 네이티브 값이 기존 앱의 값보다 먼저다.
        assert!(
            Settings::from_texts(r#"{"paddock.agentDoneSound": true}"#, r#"{"paddock.agentDoneSound": false}"#)
                .agent_done_sound()
        );
        // 불리언이 아닌 값은 설정이 없는 것으로 본다.
        assert!(Settings::from_texts(r#"{"paddock.agentDoneSound": "no"}"#, "").agent_done_sound());
    }

    #[test]
    fn test_agent_permissions_default_to_ask_and_follow_a_known_setting() {
        let permissions = |native: &str, legacy: &str| Settings::from_texts(native, legacy).agent_permissions();
        assert_eq!(permissions("", ""), Permissions::Ask);
        assert_eq!(permissions("", r#"{"paddock.agents.permissions": "skip"}"#), Permissions::Skip);
        assert_eq!(
            permissions(r#"{"paddock.agents.permissions": "allowSkip"}"#, r#"{"paddock.agents.permissions": "skip"}"#),
            Permissions::AllowSkip
        );
        assert_eq!(permissions(r#"{"paddock.agents.permissions": "always"}"#, ""), Permissions::Ask);
        assert_eq!(permissions(r#"{"paddock.agents.permissions": 3}"#, ""), Permissions::Ask);
    }

    #[test]
    fn test_c_d23_native_value_wins_over_the_old_apps_and_the_old_value_is_the_default() {
        let old =
            r#"{"window.zoomLevel": 3, "paddock.backend.onQuit": "stop", "workbench.colorTheme": "Mermaid Light"}"#;
        let native = r#"{"window.zoomLevel": 1}"#;
        let settings = Settings::from_texts(native, old);
        assert_eq!(settings.zoom_level(), 1.0);
        assert_eq!(settings.on_quit(), Some("stop"));
        assert_eq!(settings.color_theme(), Some("Mermaid Light"));
        let neither = Settings::from_texts("{oops", "[1]");
        assert_eq!((neither.zoom_level(), neither.on_quit()), (0.0, None));
    }

    #[test]
    fn test_c_d23_write_native_keeps_other_native_keys_and_never_touches_the_old_file() {
        let home = TempHome::new();
        let old_dir = home.dir.path().join(".paddock").join("config");
        std::fs::create_dir_all(&old_dir).unwrap();
        let old_text = r#"{"window.zoomLevel": 2} // keep"#;
        std::fs::write(old_dir.join("settings.json"), old_text).unwrap();
        write_native(key::ZOOM_LEVEL, Value::from(1)).unwrap();
        write_native(key::ON_QUIT, Value::from("keep")).unwrap();
        let settings = Settings::load();
        assert_eq!((settings.zoom_level(), settings.on_quit()), (1.0, Some("keep")));
        assert_eq!(std::fs::read_to_string(old_dir.join("settings.json")).unwrap(), old_text);
    }

    #[test]
    fn test_write_native_keeps_a_broken_file_aside_instead_of_losing_its_other_settings() {
        for broken in ["{ \"window.zoomLevel\": 2, oops", "[1, 2]"] {
            let _home = TempHome::new();
            std::fs::create_dir_all(native_dir()).unwrap();
            std::fs::write(native_settings_path(), broken).unwrap();
            write_native(key::ON_QUIT, Value::from("keep")).unwrap();
            let aside = native_settings_path().with_extension("json.broken");
            assert_eq!(std::fs::read_to_string(&aside).unwrap(), broken, "{broken}");
            assert_eq!(Settings::load().on_quit(), Some("keep"));
        }
    }

    #[test]
    fn test_write_native_leaves_no_temp_file_and_a_good_file_is_not_set_aside() {
        let _home = TempHome::new();
        std::fs::create_dir_all(native_dir()).unwrap();
        std::fs::write(native_settings_path(), "{\n // note\n \"window.zoomLevel\": 2,\n}").unwrap();
        write_native(key::ON_QUIT, Value::from("stop")).unwrap();
        let settings = Settings::load();
        assert_eq!((settings.zoom_level(), settings.on_quit()), (2.0, Some("stop")));
        let names: Vec<String> = std::fs::read_dir(native_dir())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["settings.json".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_native_creates_the_file_readable_by_the_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let _home = TempHome::new();
        write_native(key::ON_QUIT, Value::from("keep")).unwrap();
        assert_eq!(std::fs::metadata(native_settings_path()).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn test_write_native_treats_a_blank_file_as_empty_not_broken() {
        let _home = TempHome::new();
        std::fs::create_dir_all(native_dir()).unwrap();
        std::fs::write(native_settings_path(), "  \n").unwrap();
        write_native(key::ON_QUIT, Value::from("keep")).unwrap();
        assert!(!native_settings_path().with_extension("json.broken").exists());
        assert_eq!(Settings::load().on_quit(), Some("keep"));
    }
}
