//! 등록 계정을 "이 폴더에서 여는 계정 터미널 요청"(`Want::Launch`)으로 바꾼다.
//!
//! 실행 명세 규칙은 core의 `account_launch`가 쥐고, 여기서는 이 컴퓨터에 맞는 값(계정 폴더 위치, 설정 파일 위치, 홈 폴더,
//! Windows 여부)을 채워 넘긴다. 폴더가 없거나 다른 실행 환경의 것이면 터미널을 만들기 전에 이유를 문장으로 돌려준다.
use std::path::Path;

use paddock_core::account_launch::{
    AccountLaunchInput, Location, Permissions, account_launch_cwd, account_terminal_spec,
};
use paddock_core::agent_account::{AccountProfile, Runtime};
use paddock_core::shell_catalog::wsl_folder;
use paddock_protocol::shared_config::{config_dir, native_profile_directory};

use crate::terminal::session::Want;

/// 폴더를 못 쓸 때의 문장.
const FOLDER_UNAVAILABLE: &str =
    "The current folder is unavailable. Open an existing folder and try the account again.";

/// `profile` 계정의 터미널을 `folder`(없으면 `home`)에서 여는 요청.
///
/// WSL 계정은 Windows에서만 열 수 있고 `folder`가 같은 배포판의 WSL 폴더이거나 Windows 폴더여야 한다. 이 컴퓨터의 폴더는
/// 실제로 있어야 한다. 실패하면 화면에 그대로 보일 영어 문장이다.
///
/// Examples
/// --------
/// | 계정             | folder          | 결과                                          |
/// |------------------|-----------------|-----------------------------------------------|
/// | claude · native  | `/work`(있음)   | `/bin/bash` 시작 파일로 `/work`에서 연다      |
/// | claude · native  | `/nope`         | 오류 `The current folder is unavailable…`     |
/// | codex · WSL      | Linux 컴퓨터    | 오류 `Open this WSL account on its Windows…`  |
pub fn account_want(
    profile: &AccountProfile,
    folder: Option<&str>,
    home: &str,
    permissions: Permissions,
    is_windows: bool,
    resume_session_id: Option<&str>,
) -> Result<Want, String> {
    let folder = folder.unwrap_or(home);
    // WSL 폴더(`\\wsl.localhost\…`)와 WSL 계정의 Linux 경로는 이 컴퓨터의 파일 시스템에서 확인할 수 없고, 열 때 WSL이 확인한다.
    let is_checked_by_wsl =
        wsl_folder(folder).is_some() || (profile.runtime == Runtime::Wsl && folder.starts_with('/'));
    if is_checked_by_wsl || Path::new(folder).is_dir() {
        want_in_folder(profile, folder, home, permissions, is_windows, resume_session_id)
    } else {
        Err(FOLDER_UNAVAILABLE.to_string())
    }
}

/// 폴더를 확인한 뒤 `profile` 계정의 터미널 요청을 만든다.
fn want_in_folder(
    profile: &AccountProfile,
    folder: &str,
    home: &str,
    permissions: Permissions,
    is_windows: bool,
    resume_session_id: Option<&str>,
) -> Result<Want, String> {
    let cwd = account_launch_cwd(profile, folder, Some(home), is_windows).map_err(|error| error.to_string())?;
    let (config, flag) = match profile.runtime {
        Runtime::Native => (
            Location::Absolute(native_profile_directory(profile)),
            Location::Absolute(config_dir().join("claude").join("flag-settings.json").to_string_lossy().into_owned()),
        ),
        Runtime::Wsl => (
            Location::UnderHome(format!(".paddock/agent-profiles/{}/{}", profile.id, profile.provider)),
            Location::UnderHome(".paddock/config/claude/flag-settings.json".to_string()),
        ),
    };
    let launch = account_terminal_spec(&AccountLaunchInput {
        profile,
        config_dir: config,
        cwd: &cwd,
        is_windows,
        permissions,
        flag_settings_file: (profile.provider == "claude").then_some(flag),
        resume_session_id,
    })
    .map_err(|error| error.to_string())?;
    let (shell, wsl_distribution) = match profile.runtime {
        Runtime::Wsl => ("wsl", profile.wsl_distribution.clone()),
        Runtime::Native if is_windows => ("powershell", None),
        Runtime::Native => ("bash", None),
    };
    Ok(Want::Launch { shell: shell.to_string(), wsl_distribution, launch: Box::new(launch) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempHome;

    fn profile(
        runtime: Runtime,
        provider: &str,
    ) -> AccountProfile {
        AccountProfile {
            id: "00000000-0000-4000-8000-000000000001".into(),
            provider: provider.into(),
            label: "Main".into(),
            runtime,
            wsl_distribution: (runtime == Runtime::Wsl).then(|| "Ubuntu".to_string()),
        }
    }

    #[test]
    fn test_a_native_account_opens_in_the_given_folder_with_its_own_config_folder() {
        let home = TempHome::new();
        let folder = home.dir.path().join("work dir");
        std::fs::create_dir_all(&folder).unwrap();
        let folder = folder.to_string_lossy().into_owned();
        let want =
            account_want(&profile(Runtime::Native, "claude"), Some(&folder), &folder, Permissions::Skip, false, None);
        let Ok(Want::Launch { shell, wsl_distribution, launch }) = want else { panic!("{want:?}") };
        assert_eq!((shell.as_str(), wsl_distribution), ("bash", None));
        assert_eq!(launch.cwd.as_deref(), Some(folder.as_str()));
        assert_eq!(launch.title.as_deref(), Some("Claude \u{b7} Main"));
        let expected = native_profile_directory(&profile(Runtime::Native, "claude"));
        assert_eq!(launch.env, [("CLAUDE_CONFIG_DIR".to_string(), expected)]);
        let script = launch.args.last().unwrap().replace("'\"'\"'", "'");
        assert!(script.contains("'--dangerously-skip-permissions'"));
        assert!(script.contains("flag-settings.json"));
    }

    #[test]
    fn test_a_missing_folder_or_an_account_for_another_environment_is_refused_with_a_sentence() {
        let home = TempHome::new();
        let missing = home.dir.path().join("nope").to_string_lossy().into_owned();
        let refused = |account: &AccountProfile, folder: &str, is_windows: bool| {
            account_want(account, Some(folder), "/home", Permissions::Ask, is_windows, None).unwrap_err()
        };
        assert_eq!(refused(&profile(Runtime::Native, "claude"), &missing, false), FOLDER_UNAVAILABLE);
        let own = home.dir.path().to_string_lossy().into_owned();
        assert_eq!(
            refused(&profile(Runtime::Wsl, "codex"), &own, false),
            "Open this WSL account on its Windows device."
        );
        // WSL 안의 폴더는 이 컴퓨터에서 확인하지 않고, 다른 배포판이면 거부한다.
        assert!(
            refused(&profile(Runtime::Wsl, "codex"), r"\\wsl.localhost\Debian\home\me", true).contains("different WSL")
        );
    }

    #[test]
    fn test_c_d40_a_resumed_account_terminal_passes_only_the_conversation_id_and_a_wsl_account_accepts_a_linux_folder()
    {
        let home = TempHome::new();
        let folder = home.dir.path().to_string_lossy().into_owned();
        let session = "0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10";
        let want = account_want(
            &profile(Runtime::Native, "claude"),
            Some(&folder),
            &folder,
            Permissions::Ask,
            false,
            Some(session),
        );
        let Ok(Want::Launch { launch, .. }) = want else { panic!("{want:?}") };
        let script = launch.args.last().unwrap().replace("'\"'\"'", "'");
        assert!(script.contains(&format!("'--resume' '{session}'")), "{script}");
        // WSL 계정의 Linux 경로는 이 컴퓨터의 폴더가 아니라 WSL이 확인하므로 거부하지 않는다.
        let wsl =
            account_want(&profile(Runtime::Wsl, "claude"), Some("/home/me/app"), "~", Permissions::Ask, true, None);
        let Ok(Want::Launch { launch, .. }) = wsl else { panic!("{wsl:?}") };
        assert!(launch.args.iter().any(|argument| argument == "/home/me/app"), "{:?}", launch.args);
    }
}
