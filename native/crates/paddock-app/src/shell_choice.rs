//! 새 터미널이 쓸 기본 셸 고르기. 설정(기본 셸 프로필)이 이 컴퓨터에서 열 수 있는 셸이면 그 셸이고,
//! 아니면 이 컴퓨터에서 감지한 셸이다. Windows는 기본 WSL 배포판이 있는지 `wsl.exe`에 물어 감지한다.
use std::sync::OnceLock;

use paddock_core::settings_text::{profile_of_shell, shell_of_profile};
use paddock_core::shell_catalog::{default_shell_name, wsl_folder};

use crate::settings::Settings;
use crate::terminal::session::Want;

/// 새 터미널이 쓰는 셸과, 그 셸이 WSL일 때 쓸 배포판.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultShell {
    pub shell: &'static str,
    /// `shell`이 `wsl`일 때 터미널을 열 배포판 이름. 그 밖의 셸이면 None이다.
    pub wsl_distribution: Option<String>,
}

/// 설정이 없을 때 새 터미널이 쓰는 셸. `wsl.exe`에 한 번만 묻고 앱이 끝날 때까지 같은 값을 쓴다.
fn detected_default_shell() -> &'static DefaultShell {
    static CHOICE: OnceLock<DefaultShell> = OnceLock::new();
    CHOICE.get_or_init(|| {
        let distribution = default_distribution(installed_wsl_distributions()).cloned();
        let shell = default_shell_name(cfg!(windows), distribution.is_some());
        let wsl_distribution = if shell == "wsl" { distribution } else { None };
        DefaultShell { shell, wsl_distribution }
    })
}

/// 기본 셸 설정(프로필 이름)을 이 컴퓨터에서 열 수 있는 셸로 바꾼다. 설정이 없거나 모르는 이름이거나
/// 이 컴퓨터에 없는 셸·배포판이면 감지한 기본 셸(`detected`)이다.
///
/// Examples
/// --------
/// | 설정             | 열 수 있는 환경에 있나 | 결과                 |
/// |------------------|------------------------|----------------------|
/// | (없음)           | -                      | 감지한 기본 셸       |
/// | `Git Bash`       | 있음                   | `gitbash`            |
/// | `WSL`            | 있음                   | 감지한 기본 배포판   |
/// | `WSL:Debian`     | 없음                   | 감지한 기본 셸       |
/// | `Nushell`        | (모르는 이름)          | 감지한 기본 셸       |
pub fn chosen_default_shell(
    setting: Option<&str>,
    available: &[Environment],
    detected: &DefaultShell,
) -> DefaultShell {
    setting
        .and_then(shell_of_profile)
        .and_then(|profile| {
            // 배포판을 지정하지 않은 WSL은 이 컴퓨터의 기본 배포판이다(감지한 기본 셸이 WSL일 때 그 배포판).
            let wanted_distribution = match (profile.shell, &profile.wsl_distribution) {
                ("wsl", None) => detected.wsl_distribution.clone(),
                _ => profile.wsl_distribution.clone(),
            };
            available.iter().find(|environment| {
                environment.shell == profile.shell && environment.wsl_distribution == wanted_distribution
            })
        })
        .map(|environment| DefaultShell {
            shell: environment.shell,
            wsl_distribution: environment.wsl_distribution.clone(),
        })
        .unwrap_or_else(|| detected.clone())
}

/// 새 터미널이 쓰는 기본 셸. 설정을 부를 때마다 읽어, 빠른 설정에서 바꾼 값이 다음 새 터미널부터 쓰인다.
pub fn default_shell() -> DefaultShell {
    default_shell_with(&Settings::load())
}

/// 이미 읽어 둔 설정(`settings`)으로 정한 기본 셸. 설정 파일을 다시 읽지 않는다.
pub fn default_shell_with(settings: &Settings) -> DefaultShell {
    let detected = detected_default_shell();
    chosen_default_shell(settings.values().default_profile(), &environments(detected), detected)
}

/// 기본 셸을 설정 파일에 쓰는 프로필 이름으로 바꾼다(기존 앱과 같은 이름 체계). `default_distribution`은 이 컴퓨터의 기본 WSL 배포판이다.
pub fn profile_name_of(
    default: &DefaultShell,
    default_distribution: Option<&str>,
) -> String {
    profile_of_shell(default.shell, default.wsl_distribution.as_deref(), default_distribution)
}

/// 이 컴퓨터의 기본 WSL 배포판 이름. 배포판이 없으면 None이다.
pub fn machine_default_distribution() -> Option<&'static String> {
    default_distribution(installed_wsl_distributions())
}

/// 설치된 WSL 배포판 이름들. `wsl.exe`에 한 번만 묻고 앱이 끝날 때까지 같은 목록을 쓴다.
fn installed_wsl_distributions() -> &'static Vec<String> {
    static LIST: OnceLock<Vec<String>> = OnceLock::new();
    LIST.get_or_init(wsl_distributions)
}

/// 새 터미널을 열 수 있는 실행 환경 하나(셸 또는 WSL 배포판).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Environment {
    /// 메뉴에 보이는 이름.
    pub label: String,
    /// 이름 옆 보조 글자: 셸이 도는 곳(`Windows`·`Linux`).
    pub place: &'static str,
    /// 데몬에 넘기는 셸 이름.
    pub shell: &'static str,
    pub wsl_distribution: Option<String>,
    pub is_default: bool,
}

/// 작업 폴더(`key`)에 새 터미널을 여는 요청. WSL 공유 경로의 폴더는 그 배포판의 `wsl.exe -d 배포판 --cd 경로`로 열고,
/// 그 밖의 폴더는 기본 셸을 그 폴더에서 연다.
///
/// Examples
/// --------
/// | key                                     | 결과                                                  |
/// |-----------------------------------------|-------------------------------------------------------|
/// | `\\wsl.localhost\Ubuntu\home\me\app`  | 셸 `wsl`, 배포판 `Ubuntu`, 폴더 `/home/me/app`        |
/// | `/home/me/app`                          | 기본 셸, 폴더 `/home/me/app`                          |
pub fn folder_want(
    key: &str,
    default: &DefaultShell,
) -> Want {
    match wsl_folder(key) {
        Some((distribution, path)) => {
            Want::New { shell: "wsl".to_string(), cwd: Some(path), wsl_distribution: Some(distribution) }
        }
        None => Want::New {
            shell: default.shell.to_string(),
            cwd: Some(key.to_string()),
            wsl_distribution: default.wsl_distribution.clone(),
        },
    }
}

/// 이 컴퓨터에서 열 수 있는 실행 환경 목록(Work 머리의 ⌄ 메뉴).
///
/// Windows는 PowerShell·Command Prompt·(설치돼 있으면) Git Bash와 설치된 WSL 배포판마다 한 줄이고, 그 밖의 OS는 `bash` 하나다.
/// 컨테이너 도구의 내부 배포판은 셸을 쓸 수 없어 뺀다. 기본 환경(`default`)에는 `is_default`가 붙는다.
///
/// Examples
/// --------
/// | OS      | 배포판          | 결과 이름                                              |
/// |---------|-----------------|--------------------------------------------------------|
/// | Linux   | -               | `bash`                                                 |
/// | Windows | Ubuntu, Debian  | `PowerShell`, `Command Prompt`, `WSL · Ubuntu`, `WSL · Debian` |
pub fn environment_list(
    is_windows: bool,
    distributions: &[String],
    has_git_bash: bool,
    default: &DefaultShell,
) -> Vec<Environment> {
    let mut list = Vec::new();
    let mut push = |label: &str, place: &'static str, shell: &'static str, distribution: Option<&String>| {
        let is_default = default.shell == shell && default.wsl_distribution.as_ref() == distribution;
        list.push(Environment {
            label: label.to_string(),
            place,
            shell,
            wsl_distribution: distribution.cloned(),
            is_default,
        });
    };
    if is_windows {
        push("PowerShell", "Windows", "powershell", None);
        push("Command Prompt", "Windows", "cmd", None);
        if has_git_bash {
            push("Git Bash", "Windows", "gitbash", None);
        }
        for name in distributions.iter().filter(|name| !name.to_ascii_lowercase().starts_with("docker-desktop")) {
            push(&format!("WSL \u{b7} {name}"), "Linux", "wsl", Some(name));
        }
    } else {
        push("bash", "Linux", "bash", None);
    }
    list
}

/// 이 컴퓨터에서 열 수 있는 실행 환경 목록. 기본 환경(`default`)에 `is_default`가 붙는다.
fn environments(default: &DefaultShell) -> Vec<Environment> {
    static HAS_GIT_BASH: OnceLock<bool> = OnceLock::new();
    let has_git_bash = *HAS_GIT_BASH
        .get_or_init(|| cfg!(windows) && std::path::Path::new(r"C:\Program Files\Git\bin\bash.exe").exists());
    environment_list(cfg!(windows), installed_wsl_distributions(), has_git_bash, default)
}

/// 이 컴퓨터의 실행 환경 목록. 현재 기본 셸 설정에 맞춰 기본 환경에 표시가 붙는다.
pub fn available_environments() -> Vec<Environment> {
    environments(&default_shell())
}

/// 이미 읽어 둔 설정(`settings`)에 맞춰 기본 환경에 표시를 붙인 실행 환경 목록. 설정 파일을 다시 읽지 않는다.
pub fn available_environments_with(settings: &Settings) -> Vec<Environment> {
    environments(&default_shell_with(settings))
}

/// 설치된 WSL 배포판 이름들(`wsl.exe -l -q`의 순서 그대로, 기본 배포판이 첫 줄이다).
/// `wsl.exe`가 없거나 목록을 읽지 못하면 비어 있다.
#[cfg(windows)]
fn wsl_distributions() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    Command::new("wsl.exe")
        .args(["-l", "-q"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|output| if output.status.success() { parse_wsl_distributions(&output.stdout) } else { Vec::new() })
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn wsl_distributions() -> Vec<String> {
    Vec::new()
}

/// 터미널을 열 기본 배포판: 목록의 첫 줄. 컨테이너 도구가 만든 내부 배포판(`docker-desktop`…)은 셸을 쓸 수 없어 건너뛴다.
///
/// Examples
/// --------
/// | 목록                             | 결과       |
/// |----------------------------------|------------|
/// | `Ubuntu`, `Debian`               | `Ubuntu`   |
/// | `docker-desktop`, `Ubuntu`       | `Ubuntu`   |
/// | `docker-desktop`                 | None       |
pub fn default_distribution(names: &[String]) -> Option<&String> {
    names.iter().find(|name| !name.to_ascii_lowercase().starts_with("docker-desktop"))
}

/// `wsl.exe -l -q`의 출력에서 배포판 이름 목록을 뽑는다.
///
/// `wsl.exe`는 목록을 UTF-16LE(대개 BOM 없이)로 내보내므로 NUL 바이트가 섞여 있으면 UTF-16LE로 읽고,
/// 아니면 UTF-8로 읽는다. 빈 줄은 버린다.
///
/// Examples
/// --------
/// | 출력(글자로 풀어 쓴 것) | 결과                |
/// |-------------------------|---------------------|
/// | `Ubuntu\r\nDebian\r\n`  | `["Ubuntu", "Debian"]` |
/// | `\r\n`                  | `[]`                |
pub fn parse_wsl_distributions(output: &[u8]) -> Vec<String> {
    let is_utf16 = output.len() >= 2 && (output[..2] == [0xff, 0xfe] || output.contains(&0));
    let text = if is_utf16 {
        let units: Vec<u16> = output.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(output).into_owned()
    };
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn test_parse_wsl_list_reads_utf16_output_with_bom() {
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(utf16le("Ubuntu\r\nDebian\r\n\r\n"));
        assert_eq!(parse_wsl_distributions(&bytes), vec!["Ubuntu".to_string(), "Debian".to_string()]);
    }

    #[test]
    fn test_parse_wsl_list_reads_utf16_without_bom_and_utf8() {
        assert_eq!(parse_wsl_distributions(&utf16le("Ubuntu\r\n")), vec!["Ubuntu".to_string()]);
        assert_eq!(parse_wsl_distributions(b"Ubuntu\n"), vec!["Ubuntu".to_string()]);
    }

    #[test]
    fn test_default_distribution_is_the_first_usable_one() {
        let names = |list: &[&str]| list.iter().map(|name| name.to_string()).collect::<Vec<String>>();
        assert_eq!(default_distribution(&names(&["Ubuntu", "Debian"])), Some(&"Ubuntu".to_string()));
        assert_eq!(default_distribution(&names(&["docker-desktop", "Ubuntu"])), Some(&"Ubuntu".to_string()));
        assert_eq!(default_distribution(&names(&["docker-desktop-data"])), None);
        assert_eq!(default_distribution(&[]), None);
    }

    #[test]
    fn test_parse_wsl_list_empty_output_has_no_distribution() {
        assert!(parse_wsl_distributions(b"").is_empty());
        assert!(parse_wsl_distributions(&utf16le("\r\n")).is_empty());
    }

    #[test]
    fn test_environment_list_names_windows_shells_and_wsl_distributions_and_marks_the_default() {
        let names = |list: &[Environment]| list.iter().map(|env| env.label.clone()).collect::<Vec<_>>();
        let default = DefaultShell { shell: "wsl", wsl_distribution: Some("Ubuntu".to_string()) };
        let distributions = vec!["docker-desktop".to_string(), "Ubuntu".to_string(), "Debian".to_string()];
        let windows = environment_list(true, &distributions, true, &default);
        assert_eq!(
            names(&windows),
            vec!["PowerShell", "Command Prompt", "Git Bash", "WSL \u{b7} Ubuntu", "WSL \u{b7} Debian"]
        );
        let marked: Vec<&str> = windows.iter().filter(|env| env.is_default).map(|env| env.label.as_str()).collect();
        assert_eq!(marked, vec!["WSL \u{b7} Ubuntu"]);
        assert_eq!(windows[3].wsl_distribution.as_deref(), Some("Ubuntu"));
        assert_eq!(windows[0].place, "Windows");
        assert_eq!(windows[3].place, "Linux");
        let without_git_bash =
            environment_list(true, &[], false, &DefaultShell { shell: "powershell", wsl_distribution: None });
        assert_eq!(names(&without_git_bash), vec!["PowerShell", "Command Prompt"]);
        assert!(without_git_bash[0].is_default);
        let linux = environment_list(false, &[], false, &DefaultShell { shell: "bash", wsl_distribution: None });
        assert_eq!(names(&linux), vec!["bash"]);
        assert!(linux[0].is_default);
    }

    #[test]
    fn test_folder_want_opens_wsl_share_folders_in_their_distribution_and_others_in_the_default_shell() {
        let default = DefaultShell { shell: "powershell", wsl_distribution: None };
        assert_eq!(
            folder_want("\\\\wsl.localhost\\Ubuntu\\home\\me\\app", &default),
            Want::New {
                shell: "wsl".to_string(),
                cwd: Some("/home/me/app".to_string()),
                wsl_distribution: Some("Ubuntu".to_string())
            }
        );
        assert_eq!(
            folder_want("C:/Users/me/app", &default),
            Want::New {
                shell: "powershell".to_string(),
                cwd: Some("C:/Users/me/app".to_string()),
                wsl_distribution: None
            }
        );
    }

    #[test]
    fn test_c_d41_default_shell_follows_a_known_available_profile_and_otherwise_the_detected_one() {
        let detected = DefaultShell { shell: "powershell", wsl_distribution: None };
        let list = environment_list(true, &["Ubuntu".to_string()], true, &detected);
        let chosen = |setting: Option<&str>| {
            let found = chosen_default_shell(setting, &list, &detected);
            (found.shell, found.wsl_distribution)
        };
        assert_eq!(chosen(None), ("powershell", None));
        assert_eq!(chosen(Some("Git Bash")), ("gitbash", None));
        assert_eq!(chosen(Some("WSL:Ubuntu")), ("wsl", Some("Ubuntu".to_string())));
        // 이 컴퓨터에 없는 배포판이나 모르는 이름은 감지한 기본 셸이다.
        assert_eq!(chosen(Some("WSL:Debian")), ("powershell", None));
        assert_eq!(chosen(Some("Ubuntu (WSL)")), ("powershell", None));
        assert_eq!(chosen(Some("Nushell")), ("powershell", None));
        assert_eq!(chosen(Some("bash")), ("powershell", None));
    }

    #[test]
    fn test_c_d47_wsl_profile_without_a_distribution_means_the_machines_default_distribution() {
        let detected = DefaultShell { shell: "wsl", wsl_distribution: Some("Ubuntu".to_string()) };
        let list = environment_list(true, &["Ubuntu".to_string(), "Debian".to_string()], false, &detected);
        let chosen = |setting: &str| chosen_default_shell(Some(setting), &list, &detected).wsl_distribution;
        assert_eq!(chosen("WSL"), Some("Ubuntu".to_string()));
        assert_eq!(chosen("WSL:Debian"), Some("Debian".to_string()));
    }

    #[test]
    fn test_c_d41_environment_marks_follow_the_chosen_default_shell() {
        let detected = DefaultShell { shell: "powershell", wsl_distribution: None };
        let chosen = DefaultShell { shell: "wsl", wsl_distribution: Some("Ubuntu".to_string()) };
        let list = environment_list(true, &["Ubuntu".to_string()], false, &chosen);
        let marked: Vec<&str> = list.iter().filter(|env| env.is_default).map(|env| env.label.as_str()).collect();
        assert_eq!(marked, vec!["WSL \u{b7} Ubuntu"]);
        assert_eq!(profile_name_of(&detected, Some("Ubuntu")), "PowerShell");
        assert_eq!(profile_name_of(&chosen, Some("Ubuntu")), "WSL");
        assert_eq!(profile_name_of(&chosen, Some("Debian")), "WSL:Ubuntu");
    }
}
