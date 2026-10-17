//! 창을 닫을 때 열린 터미널을 어떻게 할지: 설정값(`onQuit`)과 그에 따른 종료 단계.
//!
//! 터미널이 하나도 없으면 설정과 무관하게 묻지 않고 닫는다. 터미널이 있으면 설정이 `ask`(기본)일 때만 사용자에게 묻고,
//! `keep`은 데몬에 남기고, `stop`은 모두 끝낸다. 설정(`paddock.backend.onQuit`)은 기존 앱과 같은 키이고,
//! 값이 없거나 깨져도 `ask`다. 설정 파일 읽기·쓰기 규칙은 `settings`가 정한다.
use std::io;

use serde_json::Value;

use crate::settings::{Settings, key, write_native};

/// 창을 닫을 때 열린 터미널에 대한 설정.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnQuit {
    /// 닫을 때마다 묻는다(기본).
    Ask,
    /// 묻지 않고 데몬에 남겨 두고 닫는다. 다시 열면 같은 터미널에 붙는다.
    Keep,
    /// 묻지 않고 모든 터미널을 끝내고 닫는다.
    Stop,
}

/// 창을 닫을 때 앱이 밟을 단계.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitStep {
    /// 열린 터미널이 없어 바로 닫는다.
    Close,
    /// 사용자에게 묻는다.
    Prompt,
    /// 터미널을 데몬에 남기고 닫는다.
    Keep,
    /// 모든 터미널을 끝내고 닫는다.
    StopAll,
}

impl OnQuit {
    /// 설정 값 글자. 파일에 쓰고 읽는 글자와 같다.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Keep => "keep",
            Self::Stop => "stop",
        }
    }

    /// 설정 값 글자에서 값을 읽는다. 값이 없거나 모르는 글자면 `Ask`다.
    ///
    /// Examples
    /// --------
    /// | 설정 값        | 결과   |
    /// |----------------|--------|
    /// | `"keep"`       | `Keep` |
    /// | `"later"`      | `Ask`  |
    /// | (없음)         | `Ask`  |
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("keep") => Self::Keep,
            Some("stop") => Self::Stop,
            _ => Self::Ask,
        }
    }
}

/// 설정과 열린 터미널 수로 정한 종료 단계.
///
/// Examples
/// --------
/// | 설정 | 터미널 수 | 결과      |
/// |------|-----------|-----------|
/// | Ask  | 0         | `Close`   |
/// | Stop | 0         | `Close`   |
/// | Ask  | 2         | `Prompt`  |
/// | Keep | 2         | `Keep`    |
/// | Stop | 2         | `StopAll` |
pub fn quit_step(
    on_quit: OnQuit,
    n_terminals: usize,
) -> QuitStep {
    match (n_terminals, on_quit) {
        (0, _) => QuitStep::Close,
        (_, OnQuit::Ask) => QuitStep::Prompt,
        (_, OnQuit::Keep) => QuitStep::Keep,
        (_, OnQuit::Stop) => QuitStep::StopAll,
    }
}

/// "Quit and Stop All Terminals"로 닫을 때의 종료 단계. 설정(`keep`·`ask`·`stop`)과 무관하게 이번 한 번만 모두 끝낸다.
/// 설정은 바꾸지 않으므로 다음 종료는 다시 설정을 따른다. 열린 터미널이 없으면 끝낼 것이 없어 그냥 닫는다.
///
/// Examples
/// --------
/// | 설정 | 터미널 수 | 결과      |
/// |------|-----------|-----------|
/// | Keep | 2         | `StopAll` |
/// | Ask  | 1         | `StopAll` |
/// | Keep | 0         | `Close`   |
pub fn quit_and_stop_all_step(n_terminals: usize) -> QuitStep {
    quit_step(OnQuit::Stop, n_terminals)
}

/// 종료 묻기에서 Enter가 고르는 기본 선택. 앱이 닫혀도 터미널이 남지 못하는 환경이면 남기는 선택이 없어 None이다.
///
/// Examples
/// --------
/// | breakaway_denied | 결과         |
/// |------------------|--------------|
/// | false            | `Some(Keep)` |
/// | true             | `None`       |
pub fn default_quit_choice(breakaway_denied: bool) -> Option<OnQuit> {
    if breakaway_denied { None } else { Some(OnQuit::Keep) }
}

/// 창을 닫을 때 state.json을 덮어써도 되는가. 터미널을 되살리기 전(연결 중·안내 단계)에는 탭이 비어 있어,
/// 저장하면 이전 상태(탭 이름·순서·선택)를 빈 목록으로 잃는다.
pub fn may_save_state_on_close(restored: bool) -> bool {
    restored
}

/// 저장된 설정을 읽는다. 네이티브 설정에 값이 있으면 그 값이, 없으면 기존 앱 설정의 값이 기본값이다.
pub fn read_on_quit() -> OnQuit {
    OnQuit::from_setting(Settings::load().on_quit())
}

/// 설정을 저장한다("Don't ask again"). 네이티브 설정 파일에만 쓴다.
pub fn write_on_quit(on_quit: OnQuit) -> io::Result<()> {
    write_native(key::ON_QUIT, Value::String(on_quit.as_str().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempHome;

    #[test]
    fn test_quit_step_follows_setting_only_when_terminals_are_open() {
        for on_quit in [OnQuit::Ask, OnQuit::Keep, OnQuit::Stop] {
            assert_eq!(quit_step(on_quit, 0), QuitStep::Close, "{on_quit:?}");
        }
        assert_eq!(quit_step(OnQuit::Ask, 2), QuitStep::Prompt);
        assert_eq!(quit_step(OnQuit::Keep, 1), QuitStep::Keep);
        assert_eq!(quit_step(OnQuit::Stop, 3), QuitStep::StopAll);
    }

    #[test]
    fn test_from_setting_falls_back_to_ask() {
        assert_eq!(OnQuit::from_setting(Some("keep")), OnQuit::Keep);
        assert_eq!(OnQuit::from_setting(Some("stop")), OnQuit::Stop);
        for value in [Some("later"), Some(""), None] {
            assert_eq!(OnQuit::from_setting(value), OnQuit::Ask, "{value:?}");
        }
    }

    #[test]
    fn test_enter_has_no_default_when_terminals_cannot_outlive_the_app() {
        assert_eq!(default_quit_choice(false), Some(OnQuit::Keep));
        assert_eq!(default_quit_choice(true), None);
    }

    #[test]
    fn test_state_is_saved_on_close_only_after_the_terminals_were_restored() {
        // 연결 중이거나 안내 단계에서 닫으면 탭이 비어 있다. 그 빈 목록으로 이전 state.json을 덮으면 안 된다.
        assert!(may_save_state_on_close(true));
        assert!(!may_save_state_on_close(false));
    }

    #[test]
    fn test_setting_roundtrips_under_the_shared_key_and_old_apps_value_is_the_default() {
        let home = TempHome::new();
        assert_eq!(read_on_quit(), OnQuit::Ask, "no file means ask");
        let old_dir = home.dir.path().join(".paddock").join("config");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(old_dir.join("settings.json"), "{\n  \"paddock.backend.onQuit\": \"stop\",\n}").unwrap();
        assert_eq!(read_on_quit(), OnQuit::Stop, "the old app's value is the default");
        write_on_quit(OnQuit::Keep).unwrap();
        assert_eq!(read_on_quit(), OnQuit::Keep, "the native value wins");
        let text = std::fs::read_to_string(paddock_protocol::daemon_info::native_dir().join("settings.json")).unwrap();
        assert!(text.contains("paddock.backend.onQuit"), "{text}");
    }

    #[test]
    fn test_c_d41_quit_and_stop_all_ignores_keep_setting_once() {
        let home = TempHome::new();
        write_on_quit(OnQuit::Keep).unwrap();
        let settings_path = paddock_protocol::daemon_info::native_dir().join("settings.json");
        let before = std::fs::read_to_string(&settings_path).unwrap();
        // 같은 열린 터미널 수에서 설정 `keep`은 남기고, "Quit and Stop All"은 설정과 무관하게 모두 끝낸다.
        assert_eq!(quit_step(read_on_quit(), 2), QuitStep::Keep);
        assert_eq!(quit_and_stop_all_step(2), QuitStep::StopAll);
        // 터미널이 없으면 끝낼 것이 없어 그냥 닫는다.
        assert_eq!(quit_and_stop_all_step(0), QuitStep::Close);
        // 이번 한 번만이다: 설정은 그대로 `keep`이고 다음 종료는 다시 남긴다.
        assert_eq!(std::fs::read_to_string(&settings_path).unwrap(), before);
        assert_eq!(quit_step(read_on_quit(), 2), QuitStep::Keep);
        drop(home);
    }
}
