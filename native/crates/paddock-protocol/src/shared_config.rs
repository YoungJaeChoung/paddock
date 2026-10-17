//! 기존 앱과 함께 쓰는 설정 폴더(`~/.paddock/config`)의 위치와 계정 목록 읽기.
//!
//! 네이티브 앱과 데몬은 이 폴더를 읽기만 한다. 파일 형식과 쓰기 규칙은 두 앱이 같은 파일을 공유하는 계약이므로 기존 앱과 같다.
use std::path::PathBuf;

use paddock_core::agent_account::{AccountProfile, parse_account_profiles};
use paddock_core::settings_text::SettingValues;
use paddock_core::terminal_history::directory_name;

use crate::daemon_info::native_dir;

/// 기존 앱과 공유하는 설정 폴더(`~/.paddock/config`).
pub fn config_dir() -> PathBuf {
    std::env::home_dir().expect("a home directory is required").join(".paddock").join("config")
}

/// 계정 목록 파일(`account-profiles.json`)의 경로.
pub fn account_profiles_path() -> PathBuf {
    config_dir().join("account-profiles.json")
}

/// 네이티브 설정 파일(`~/.paddock/native/settings.json`)의 경로.
pub fn native_settings_path() -> PathBuf {
    native_dir().join("settings.json")
}

/// 기존 앱 설정 파일(`~/.paddock/config/settings.json`)의 경로. 읽기만 한다.
pub fn legacy_settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// 두 설정 파일을 읽는다. 파일이 없거나 읽을 수 없으면 그 파일의 설정은 없는 것으로 본다.
pub fn read_settings() -> SettingValues {
    let read = |path: PathBuf| std::fs::read_to_string(path).unwrap_or_default();
    SettingValues::from_texts(&read(native_settings_path()), &read(legacy_settings_path()))
}

/// 터미널 기록 폴더(`~/.paddock/native/terminal-history`). 데몬마다 하위 폴더 하나를 쓴다.
pub fn history_root() -> PathBuf {
    native_dir().join("terminal-history")
}

/// 식별 글자가 `daemon_identity`(`pid@시작 시각`)인 데몬의 기록 폴더. 터미널마다 `<번호>.json` 하나가 든다.
pub fn history_dir(daemon_identity: &str) -> PathBuf {
    history_root().join(directory_name(daemon_identity))
}

/// 이 컴퓨터(네이티브)에서 본 계정 폴더: `<설정 폴더>/agent-profiles/<계정 id>/<도구>`.
pub fn native_profile_directory(profile: &AccountProfile) -> String {
    config_dir().join("agent-profiles").join(&profile.id).join(&profile.provider).to_string_lossy().into_owned()
}

/// 등록된 계정 목록. 파일이 없거나 깨졌으면 빈 목록이다. 계정 폴더 안은 읽지 않는다.
pub fn read_account_profiles() -> Vec<AccountProfile> {
    std::fs::read_to_string(account_profiles_path()).map(|text| parse_account_profiles(&text)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::TempHome;
    use paddock_core::agent_account::Runtime;

    #[test]
    fn test_account_profiles_are_read_from_the_shared_config_folder_and_missing_file_is_empty() {
        let home = TempHome::new();
        assert!(read_account_profiles().is_empty());
        std::fs::create_dir_all(config_dir()).unwrap();
        std::fs::write(
            account_profiles_path(),
            r#"{"version":1,"profiles":[{"id":"abc","provider":"claude","label":"Main","runtime":"native"}]}"#,
        )
        .unwrap();
        let profiles = read_account_profiles();
        assert_eq!(profiles.len(), 1);
        assert_eq!((profiles[0].runtime, profiles[0].label.as_str()), (Runtime::Native, "Main"));
        let directory = native_profile_directory(&profiles[0]);
        assert!(directory.starts_with(&home.dir.path().to_string_lossy().into_owned()));
        assert!(directory.ends_with("agent-profiles/abc/claude") || directory.ends_with("agent-profiles\\abc\\claude"));
    }
}
