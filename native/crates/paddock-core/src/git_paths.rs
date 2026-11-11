//! git을 어디서 어떻게 실행할지 정하는 순수 규칙: WSL 폴더 판정, 인자·출력의 경로 표기 변환, 실행 명령줄, 환경 변수.
//!
//! WSL 배포판 폴더(`\\wsl.localhost\<배포판>\…`·`\\wsl$\<배포판>\…`)의 저장소를 Windows git이 9P 위에서 열면 WSL 쪽 git이
//! index·pack을 바꾸는 순간 파일 크기를 못 읽어 죽는다. 그래서 WSL 폴더는 그 배포판 안의 git을 `wsl.exe`로 실행하고,
//! Windows git으로 되돌아가지 않는다. 프로세스를 띄우는 일은 앱이 하고, 여기서는 무엇을 띄울지만 정한다.

/// WSL 폴더를 Windows에서 가리키는 호스트 이름 두 가지.
const WSL_HOSTS: [&str; 2] = ["wsl.localhost", "wsl$"];

/// 환경 변수 중 WSL 안 git에 넘기지 않는 것. Windows 경로의 스크립트나 Windows 프로세스끼리의 통신을 가정해 WSL에서 동작할 수 없다.
const NOT_FORWARDED: [&str; 3] = ["GIT_ASKPASS", "SSH_ASKPASS", "ELECTRON_RUN_AS_NODE"];

/// git을 실행할 곳.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitTarget {
    /// 이 컴퓨터의 git을 `cwd`에서 실행한다.
    Local { cwd: String },
    /// 배포판 안 git을 `wsl.exe`로 실행한다. `host`는 들어온 표기(`wsl.localhost` 또는 `wsl$`)를 그대로 둔다.
    Wsl { host: String, distro: String, linux_cwd: String, windows_cwd: String },
}

fn is_separator(c: char) -> bool {
    c == '\\' || c == '/'
}

/// `\\wsl.localhost\Ubuntu\home\me` 같은 경로에서 (호스트 표기, 배포판, Linux 경로)를 꺼낸다. WSL 경로가 아니면 None이다.
///
/// Examples
/// --------
/// | 경로                              | 결과                                   |
/// |-----------------------------------|----------------------------------------|
/// | `\\wsl.localhost\Ubuntu\home\me`  | (`wsl.localhost`, `Ubuntu`, `/home/me`) |
/// | `//wsl$/Debian`                   | (`wsl$`, `Debian`, `/`)                |
/// | `C:\Users\me`                     | None                                   |
pub fn parse_wsl_path(path: &str) -> Option<(String, String, String)> {
    let chars: Vec<char> = path.chars().collect();
    let mut found = None;
    if chars.len() > 2 && is_separator(chars[0]) && is_separator(chars[1]) {
        let host_end = chars[2..].iter().position(|c| is_separator(*c)).map_or(chars.len(), |at| at + 2);
        let host: String = chars[2..host_end].iter().collect();
        let is_wsl_host = WSL_HOSTS.iter().any(|candidate| candidate.eq_ignore_ascii_case(&host));
        if is_wsl_host && host_end < chars.len() {
            let distro_end =
                chars[host_end + 1..].iter().position(|c| is_separator(*c)).map_or(chars.len(), |at| at + host_end + 1);
            let distro: String = chars[host_end + 1..distro_end].iter().collect();
            if !distro.is_empty() {
                let rest: String = chars[distro_end..].iter().collect::<String>().replace('\\', "/");
                found = Some((host, distro, if rest.is_empty() { "/".to_string() } else { rest }));
            }
        }
    }
    found
}

/// 폴더에서 git을 어디서 실행할지 정한다. WSL 폴더면 `Wsl`, 그 밖에는 `Local`이다.
pub fn target_of(folder: &str) -> GitTarget {
    match parse_wsl_path(folder) {
        Some((host, distro, linux_cwd)) => {
            GitTarget::Wsl { host, distro, linux_cwd, windows_cwd: folder.replace('/', "\\") }
        }
        None => GitTarget::Local { cwd: folder.to_string() },
    }
}

impl GitTarget {
    /// WSL이면 git이 Windows에서 내는 꼴의 루트(`//wsl.localhost/Ubuntu`). 출력의 Linux 경로를 되돌리는 데 쓴다.
    pub fn wsl_root(&self) -> Option<String> {
        match self {
            Self::Wsl { host, distro, .. } => Some(format!("//{host}/{distro}")),
            Self::Local { .. } => None,
        }
    }
}

fn is_drive_path(text: &str) -> bool {
    let chars: Vec<char> = text.chars().take(3).collect();
    chars.len() == 3 && chars[0].is_ascii_alphabetic() && chars[1] == ':' && is_separator(chars[2])
}

/// 인자(또는 `--옵션=값`의 값)가 Windows 경로면 Linux 경로로 바꾼다. 같은 배포판의 WSL 경로와 드라이브 경로만 바꾸고 나머지는 그대로다.
///
/// Examples
/// --------
/// | 입력                                   | distro  | 결과                       |
/// |----------------------------------------|---------|----------------------------|
/// | `\\wsl.localhost\Ubuntu\home\me\a.txt` | Ubuntu  | `/home/me/a.txt`           |
/// | `C:\Users\me\x.patch`                  | Ubuntu  | `/mnt/c/Users/me/x.patch`  |
/// | `--git-dir=C:/repo/.git`               | Ubuntu  | `--git-dir=/mnt/c/repo/.git` |
/// | `\\wsl.localhost\Debian\a`             | Ubuntu  | 그대로(다른 배포판)        |
/// | `status`·`-z`·`HEAD:C:x`               | Ubuntu  | 그대로                     |
pub fn to_linux_argument(
    argument: &str,
    distro: &str,
) -> String {
    // `--옵션=값`은 값만 본다. 경로 자체에 든 `=`는 옵션 구분으로 보지 않는다.
    let is_path = |text: &str| is_drive_path(text) || text.starts_with("\\\\") || text.starts_with("//");
    let (prefix, value) = match argument.split_once('=') {
        Some((name, value)) if !is_path(argument) && !name.is_empty() => (format!("{name}="), value),
        _ => (String::new(), argument),
    };
    let converted = if is_drive_path(value) {
        let mut chars = value.chars();
        let letter = chars.next().map_or('c', |c| c.to_ascii_lowercase());
        let rest: String = chars.skip(1).collect::<String>().replace('\\', "/");
        Some(format!("/mnt/{letter}{rest}"))
    } else {
        parse_wsl_path(value)
            .filter(|(_, path_distro, _)| path_distro.eq_ignore_ascii_case(distro))
            .map(|(_, _, linux)| linux)
    };
    match converted {
        Some(converted) => format!("{prefix}{converted}"),
        None => argument.to_string(),
    }
}

/// git 출력에 든 Linux 절대 경로를 Windows git이 같은 저장소에서 내는 꼴로 되돌린다. 절대 경로가 아니면 그대로다.
///
/// Examples
/// --------
/// | 입력                 | wsl_root                 | 결과                                  |
/// |----------------------|--------------------------|---------------------------------------|
/// | `/home/me/repo`      | `//wsl.localhost/Ubuntu` | `//wsl.localhost/Ubuntu/home/me/repo` |
/// | `/home/me/repo`      | `//wsl$/Ubuntu`          | `//wsl$/Ubuntu/home/me/repo`          |
/// | `/mnt/c/Users/me/r`  | `//wsl.localhost/Ubuntu` | `C:/Users/me/r`                       |
/// | `.git`               | `//wsl.localhost/Ubuntu` | `.git`                                |
pub fn to_windows_path(
    linux_path: &str,
    wsl_root: &str,
) -> String {
    let mut converted = linux_path.to_string();
    if linux_path.starts_with('/') {
        let drive = linux_path.strip_prefix("/mnt/").and_then(|rest| {
            let mut chars = rest.chars();
            let letter = chars.next().filter(|c| c.is_alphabetic())?;
            let tail = chars.as_str();
            (tail.is_empty() || tail.starts_with('/'))
                .then(|| format!("{}:{}", letter.to_ascii_uppercase(), if tail.is_empty() { "/" } else { tail }))
        });
        converted = drive.unwrap_or_else(|| format!("{wsl_root}{linux_path}"));
    }
    converted
}

/// 줄 단위 출력의 경로 줄을 Windows 꼴로 바꾼다. `worktree <경로>` 접두가 붙은 줄도 다룬다.
pub fn to_windows_output(
    text: &str,
    wsl_root: &str,
) -> String {
    text.split('\n')
        .map(|line| {
            let (body, tail) = line.strip_suffix('\r').map_or((line, ""), |body| (body, "\r"));
            let converted = match body.strip_prefix("worktree ") {
                Some(path) if path.starts_with('/') => format!("worktree {}", to_windows_path(path, wsl_root)),
                _ => to_windows_path(body, wsl_root),
            };
            format!("{converted}{tail}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// git 명령의 프로그램과 인자. Local은 `git args…`(실행 폴더는 호출자가 `cwd`로 준다), Wsl은
/// `wsl.exe -d <배포판> --cd <Windows 폴더> -e git args…`이다.
///
/// 인자는 어느 쪽이든 그대로 넘긴다. 앱이 넘기는 경로는 모두 저장소 상대 경로이고 커밋 메시지는 사용자가 쓴 글이라, `C:\x` 꼴이라고
/// 바꾸면 메시지가 변형된다. Windows 절대 경로를 넘겨야 하는 명령이 생기면 그 인자만 `to_linux_argument`로 직접 바꿔 넘긴다.
pub fn command(
    target: &GitTarget,
    args: &[String],
) -> (String, Vec<String>) {
    match target {
        GitTarget::Local { .. } => ("git".to_string(), args.to_vec()),
        GitTarget::Wsl { distro, windows_cwd, .. } => {
            let mut full: Vec<String> =
                ["-d", distro, "--cd", windows_cwd, "-e", "git"].iter().map(|part| part.to_string()).collect();
            full.extend(args.iter().cloned());
            ("wsl.exe".to_string(), full)
        }
    }
}

/// git 프로세스에 줄 환경 변수. 터미널 입력 묻기를 끄고(`GIT_TERMINAL_PROMPT=0`), status가 index를 쓰지 않게 하고(`GIT_OPTIONAL_LOCKS=0`),
/// 오류 문구를 글로 판정하므로 지역화하지 않는다(`LC_ALL=C`). WSL이면 `WSLENV`에 세 이름을 덧붙여 `wsl.exe`가 배포판 안 git에 넘기게 한다. `inherited_wslenv`는 앱이 이미 가진 값이며,
/// 그 안의 askpass·편집기 계열 이름은 뺀다.
pub fn environment(
    target: &GitTarget,
    inherited_wslenv: Option<&str>,
) -> Vec<(String, String)> {
    let mut env = vec![
        ("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()),
        ("GIT_OPTIONAL_LOCKS".to_string(), "0".to_string()),
        ("LC_ALL".to_string(), "C".to_string()),
    ];
    if matches!(target, GitTarget::Wsl { .. }) {
        let mut entries: Vec<String> = inherited_wslenv
            .unwrap_or("")
            .split(':')
            .filter(|entry| {
                let name = entry.split('/').next().unwrap_or("");
                !entry.is_empty() && !NOT_FORWARDED.contains(&name) && !name.starts_with("VSCODE_GIT_")
            })
            .map(str::to_string)
            .collect();
        entries.extend(["GIT_TERMINAL_PROMPT/u", "GIT_OPTIONAL_LOCKS/u", "LC_ALL/u"].map(str::to_string));
        env.push(("WSLENV".to_string(), entries.join(":")));
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn test_c_git_1_a_wsl_unc_path_in_the_same_distro_becomes_a_linux_path() {
        assert_eq!(to_linux_argument(r"\\wsl.localhost\Ubuntu\home\me\a b.txt", "Ubuntu"), "/home/me/a b.txt");
    }

    #[test]
    fn test_c_git_2_a_drive_path_becomes_a_mnt_path() {
        assert_eq!(to_linux_argument(r"C:\Users\me\x y.patch", "Ubuntu"), "/mnt/c/Users/me/x y.patch");
        assert_eq!(to_linux_argument("D:/data/f", "Ubuntu"), "/mnt/d/data/f");
    }

    #[test]
    fn test_c_git_3_an_option_value_is_converted_and_the_option_name_is_kept() {
        assert_eq!(to_linux_argument("--git-dir=C:/repo/.git", "Ubuntu"), "--git-dir=/mnt/c/repo/.git");
    }

    #[test]
    fn test_c_git_4_a_path_in_another_distro_stays_as_it_is() {
        let other = r"\\wsl.localhost\Debian\a";
        assert_eq!(to_linux_argument(other, "Ubuntu"), other);
    }

    #[test]
    fn test_c_git_5_words_that_only_look_like_paths_stay_as_they_are() {
        for word in ["status", "-z", "HEAD:C:x", "C:", "a=b", ""] {
            assert_eq!(to_linux_argument(word, "Ubuntu"), word, "{word}");
        }
    }

    #[test]
    fn test_c_git_6_an_absolute_linux_path_gets_the_wsl_root_in_the_notation_of_the_folder() {
        assert_eq!(to_windows_path("/home/me/repo", "//wsl.localhost/Ubuntu"), "//wsl.localhost/Ubuntu/home/me/repo");
        assert_eq!(to_windows_path("/home/me/repo", "//wsl$/Ubuntu"), "//wsl$/Ubuntu/home/me/repo");
    }

    #[test]
    fn test_c_git_7_a_mnt_drive_path_becomes_a_drive_path() {
        assert_eq!(to_windows_path("/mnt/c/Users/me/r", "//wsl.localhost/Ubuntu"), "C:/Users/me/r");
        assert_eq!(to_windows_path("/mnt/c", "//wsl.localhost/Ubuntu"), "C:/");
    }

    #[test]
    fn test_c_git_8_a_relative_path_is_not_touched() {
        assert_eq!(to_windows_path(".git", "//wsl.localhost/Ubuntu"), ".git");
    }

    #[test]
    fn test_c_git_9_output_lines_are_converted_one_by_one_including_the_worktree_prefix() {
        let root = "//wsl.localhost/Ubuntu";
        assert_eq!(
            to_windows_output("/home/me/r\r\n.git\nworktree /home/me/w\n", root),
            "//wsl.localhost/Ubuntu/home/me/r\r\n.git\nworktree //wsl.localhost/Ubuntu/home/me/w\n"
        );
    }

    #[test]
    fn test_c_git_shim_a_wsl_folder_runs_git_inside_its_distribution_and_other_folders_run_locally() {
        let wsl = target_of(r"\\wsl.localhost\Ubuntu\home\me\repo");
        assert_eq!(
            wsl,
            GitTarget::Wsl {
                host: "wsl.localhost".into(),
                distro: "Ubuntu".into(),
                linux_cwd: "/home/me/repo".into(),
                windows_cwd: r"\\wsl.localhost\Ubuntu\home\me\repo".into(),
            }
        );
        assert_eq!(target_of("C:/Users/me/repo"), GitTarget::Local { cwd: "C:/Users/me/repo".into() });
        assert_eq!(target_of("/home/me/repo"), GitTarget::Local { cwd: "/home/me/repo".into() });
    }

    #[test]
    fn test_c_git_shim_the_wsl_command_line_passes_every_argument_unchanged_after_the_git_name() {
        let target = target_of(r"\\wsl.localhost\Ubuntu\home\me");
        let odd =
            args(&["a b", "한글 파일.txt", "say \"hi\"", "back\\slash", "trail\\", "", "$HOME", "*", "it's", "--"]);
        let (program, full) = command(&target, &odd);
        assert_eq!(program, "wsl.exe");
        let head = args(&["-d", "Ubuntu", "--cd", r"\\wsl.localhost\Ubuntu\home\me", "-e", "git"]);
        assert_eq!(&full[..6], head.as_slice());
        assert_eq!(&full[6..], odd.as_slice());
    }

    #[test]
    fn test_c_git_shim_wsl_arguments_pass_unchanged_even_when_they_look_like_windows_paths() {
        // 앱이 넘기는 경로는 모두 저장소 상대 경로이고 커밋 메시지는 사용자가 쓴 글이다 — 어느 쪽도 바뀌면 안 된다.
        let target = target_of(r"\\wsl.localhost\Ubuntu\home\me");
        let odd = args(&[
            "commit",
            "-m",
            r"fix path=C:\x",
            "D:/data/f",
            r"C:\Users\me\x.patch",
            r"\\wsl.localhost\Ubuntu\home\me\a",
            r"x=C:\y",
            "--git-dir=C:/r/.git",
        ]);
        let (_, full) = command(&target, &odd);
        assert_eq!(&full[6..], odd.as_slice());
    }

    #[test]
    fn test_c_git_shim_a_local_command_is_git_with_the_arguments_as_they_are() {
        let target = target_of("C:/Users/me/repo");
        assert_eq!(command(&target, &args(&["status", "-z"])), ("git".to_string(), args(&["status", "-z"])));
    }

    #[test]
    fn test_c_git_shim_output_keeps_the_notation_of_the_folder() {
        assert_eq!(target_of(r"\\wsl$\Ubuntu\home\me").wsl_root().as_deref(), Some("//wsl$/Ubuntu"));
        assert_eq!(target_of(r"\\wsl.localhost\Ubuntu\home\me").wsl_root().as_deref(), Some("//wsl.localhost/Ubuntu"));
        assert_eq!(target_of("/home/me").wsl_root(), None);
    }

    #[test]
    fn test_c_git_shim_terminal_prompts_are_off_and_optional_locks_are_off_for_both_targets() {
        for target in [target_of("/home/me"), target_of(r"\\wsl.localhost\Ubuntu\home\me")] {
            let env = environment(&target, None);
            assert!(env.contains(&("GIT_TERMINAL_PROMPT".into(), "0".into())));
            assert!(env.contains(&("GIT_OPTIONAL_LOCKS".into(), "0".into())));
            assert!(
                env.contains(&("LC_ALL".into(), "C".into())),
                "messages are matched as text, so they must not be localized"
            );
        }
    }

    #[test]
    fn test_c_git_shim_wslenv_forwards_the_two_names_and_drops_askpass_names_from_an_inherited_list() {
        let wsl = target_of(r"\\wsl.localhost\Ubuntu\home\me");
        let env = environment(&wsl, Some("GIT_ASKPASS:KEEP/p:SSH_ASKPASS:VSCODE_GIT_IPC_HANDLE:ELECTRON_RUN_AS_NODE"));
        let wslenv = env.iter().find(|(name, _)| name == "WSLENV").map(|(_, value)| value.as_str()).unwrap();
        assert_eq!(wslenv, "KEEP/p:GIT_TERMINAL_PROMPT/u:GIT_OPTIONAL_LOCKS/u:LC_ALL/u");
        assert!(environment(&target_of("/home/me"), Some("A")).iter().all(|(name, _)| name != "WSLENV"));
    }

    #[test]
    fn test_c_git_shim_wsl_hosts_are_recognised_in_either_spelling_and_a_bare_host_is_not_a_folder() {
        assert!(parse_wsl_path("//WSL$/Ubuntu/x").is_some());
        assert_eq!(
            parse_wsl_path(r"\\wsl.localhost\Ubuntu"),
            Some(("wsl.localhost".into(), "Ubuntu".into(), "/".into()))
        );
        assert_eq!(parse_wsl_path(r"\\wsl.localhost"), None);
        assert_eq!(parse_wsl_path(r"\\server\share"), None);
        assert!(NOT_FORWARDED.contains(&"GIT_ASKPASS"));
    }
}
