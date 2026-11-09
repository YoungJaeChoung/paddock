//! 일반 터미널에서 친 `claude`에 Paddock 전용 설정(`--settings <플래그 파일>`)이 붙게 하는 실행 래퍼의 본문.
//!
//! 계정 터미널은 시작 명령에 `--settings`를 직접 넣지만, 일반 터미널은 사용자가 `claude`를 직접 치므로
//! 설정 폴더의 `bin` 폴더를 터미널 PATH 맨 앞에 두고(`claude_wrapper_path`) 거기에 같은 이름의 래퍼를 놓는다.
//! 래퍼는 자기 폴더를 PATH에서 뺀 뒤 진짜 `claude`를 `--settings`와 함께 실행하고, 나머지 인자는 그대로 넘긴다.
//! 플래그 파일이 없거나 첫 인자가 `remote-control`이면 인자를 붙이지 않는다. 기존 앱과 같은 글자여야 두 앱이 서로 덮어쓰지 않으므로 본문은 글자 그대로 옮겼다.

/// 래퍼 폴더와 파일 이름.
pub struct ClaudeWrapper;

impl ClaudeWrapper {
    /// 설정 폴더 아래 래퍼 폴더 이름.
    pub const DIRECTORY: &'static str = "bin";
    /// `~/.bashrc`에 래퍼 폴더를 PATH 앞에 넣을 때 쓰는 표지. 이 표지가 든 줄만 Paddock이 넣은 것으로 알아본다.
    pub const BASHRC_MARKER: &'static str = "# paddock-claude-wrapper";
}

/// 래퍼의 종류. `Sh`는 Linux·macOS·WSL용, `Cmd`는 Windows 명령 프롬프트·PowerShell용이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapperKind {
    Sh,
    Cmd,
}

impl WrapperKind {
    /// 래퍼 폴더 안 파일 이름.
    pub fn file_name(self) -> &'static str {
        match self {
            WrapperKind::Sh => "claude",
            WrapperKind::Cmd => "claude.cmd",
        }
    }
}

/// 값을 POSIX 셸 한 단어로 감싼다. 작은따옴표는 닫고 이스케이프한 뒤 다시 연다.
pub fn shell_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// 래퍼 스크립트 본문.
///
/// `own_directory`는 래퍼 폴더의 경로다(cmd 래퍼가 PATH에서 자기 폴더를 뺄 때 쓴다. sh 래퍼는 실행 위치로 스스로 안다).
///
/// Examples
/// --------
/// | kind | 하는 일                                                                                                  |
/// |------|----------------------------------------------------------------------------------------------------------|
/// | Sh   | 자기 폴더를 PATH에서 뺌 → 플래그 파일이 있으면 `exec claude --settings <파일> "$@"`, 없으면 `exec claude "$@"` |
/// | Cmd  | 자기 폴더를 PATH에서 뺌 → 같은 규칙으로 `claude --settings "<파일>" %*`                                 |
/// | 둘 다 | `claude remote-control ...`은 플래그 파일이 있어도 `--settings` 없이 그대로 넘긴다                       |
pub fn claude_wrapper_script(
    kind: WrapperKind,
    flag_file: &str,
    own_directory: &str,
) -> String {
    match kind {
        WrapperKind::Sh => [
            "#!/bin/sh",
            "# Paddock가 만든 claude 실행 래퍼. Paddock 전용 설정(상태 줄·메모리 안내)을 --settings로 덧붙이고 진짜 claude를 실행한다.",
            // PATH를 앞에서부터 훑어 자기 자신이 아닌 첫 claude를 찾는다. 같은 파일인지는 경로 표기(끝 슬래시·..)가 아니라 파일 동일성(-ef)으로 가려,
            // 래퍼 폴더가 다른 표기로 PATH에 남아 있어도 자기 자신을 다시 실행하는 무한 반복이 생기지 않는다. 빈 PATH 조각은 건너뛴다.
            "real=",
            "old_ifs=$IFS",
            "IFS=:",
            "for dir in $PATH; do",
            "    [ -n \"$dir\" ] || continue",
            "    if [ -f \"$dir/claude\" ] && [ -x \"$dir/claude\" ] && ! [ \"$dir/claude\" -ef \"$0\" ]; then real=\"$dir/claude\"; break; fi",
            "done",
            "IFS=$old_ifs",
            "if [ -z \"$real\" ]; then printf '%s\\n' \"claude was not found in PATH (Paddock's launcher found only itself). Install Claude Code, then try again.\" >&2; exit 127; fi",
            &format!("flag={}", shell_word(flag_file)),
            // `claude remote-control`(원격 접속 서버)은 하위 명령 앞의 `--settings`를 자기가 띄우는 세션에 넘길 수 없다며 시작을 거부한다.
            // 그래서 이 명령일 때만 플래그 파일을 붙이지 않는다. 대화 세션의 `--remote-control`·`/remote-control`은 `--settings`와 함께 동작한다.
            "if [ \"$1\" != remote-control ] && [ -f \"$flag\" ]; then exec \"$real\" --settings \"$flag\" \"$@\"; fi",
            "exec \"$real\" \"$@\"",
            "",
        ]
        .join("\n"),
        WrapperKind::Cmd => [
            "@echo off",
            "rem Paddock가 만든 claude 실행 래퍼. Paddock 전용 설정(상태 줄·메모리 안내)을 --settings로 덧붙이고 진짜 claude를 실행한다.",
            "setlocal",
            &format!("set \"PATH=%PATH:{own_directory};=%\""),
            &format!("set \"PATH=%PATH:;{own_directory}=%\""),
            // 위에서 못 뺀 다른 표기의 자기 폴더가 PATH에 남아도 자기 자신을 다시 실행하지 않도록, 찾은 claude가 이 파일이면 멈춘다.
            "set \"real=\"",
            "for /f \"delims=\" %%i in ('where claude 2^>nul') do if not defined real if /i not \"%%~fi\"==\"%~f0\" set \"real=%%i\"",
            "if not defined real (",
            "    echo claude was not found in PATH ^(Paddock launcher found only itself^). Install Claude Code, then try again. 1>&2",
            "    exit /b 9009",
            ")",
            // sh 래퍼와 같은 이유로 `claude remote-control`에는 --settings를 붙이지 않는다.
            "if /i \"%~1\"==\"remote-control\" goto plain",
            &format!("if not exist \"{flag_file}\" goto plain"),
            &format!("\"%real%\" --settings \"{flag_file}\" %*"),
            "exit /b %ERRORLEVEL%",
            ":plain",
            "\"%real%\" %*",
            "exit /b %ERRORLEVEL%",
            "",
        ]
        .join("\r\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::process::Command;

    #[cfg(unix)]
    fn run_sh(args: &[&str]) -> Option<std::process::Output> {
        Command::new("sh").args(args).output().ok()
    }

    #[cfg(unix)]
    #[test]
    fn test_c_wrapper_1_the_sh_script_passes_a_shell_syntax_check() {
        let script = claude_wrapper_script(WrapperKind::Sh, "/tmp/it's flag.json", "");
        let output = run_sh(&["-n", "-c", &script]).expect("sh runs");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }

    #[cfg(unix)]
    mod running {
        use super::*;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        fn folder(name: &str) -> std::path::PathBuf {
            let root = std::env::temp_dir().join(format!("claude wrapper {name} {}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            root
        }

        fn executable(
            path: &std::path::Path,
            text: &str,
        ) {
            fs::write(path, text).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        fn fake_claude(directory: &std::path::Path) {
            fs::create_dir_all(directory).unwrap();
            executable(&directory.join("claude"), "#!/bin/sh\nprintf \"REAL|%s\\n\" \"$*\"\nexit 7\n");
        }

        #[test]
        fn test_c_wrapper_2_the_wrapper_skips_its_own_folder_runs_the_real_claude_with_the_flag_file_and_keeps_the_exit_code()
         {
            let root = folder("two");
            let bin = root.join("config").join("bin");
            let real = root.join("real bin");
            fake_claude(&real);
            let flag_file = root.join("flag's settings.json");
            fs::write(&flag_file, "{}").unwrap();
            fs::create_dir_all(&bin).unwrap();
            let wrapper = bin.join("claude");
            executable(&wrapper, &claude_wrapper_script(WrapperKind::Sh, flag_file.to_str().unwrap(), ""));
            let path = format!("{}:{}:/usr/bin:/bin", bin.display(), real.display());
            let output =
                Command::new(&wrapper).args(["-p", "two words", "--model", "x"]).env("PATH", &path).output().unwrap();
            assert_eq!(output.status.code(), Some(7));
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                format!("REAL|--settings {} -p two words --model x", flag_file.display())
            );
            let _ = fs::remove_dir_all(&root);
        }

        #[test]
        fn test_c_wrapper_3_without_a_flag_file_the_arguments_pass_through_unchanged() {
            let root = folder("three");
            let bin = root.join("bin");
            let real = root.join("real");
            fake_claude(&real);
            fs::create_dir_all(&bin).unwrap();
            let wrapper = bin.join("claude");
            executable(
                &wrapper,
                &claude_wrapper_script(WrapperKind::Sh, root.join("missing.json").to_str().unwrap(), ""),
            );
            let path = format!("{}:{}:/usr/bin:/bin", bin.display(), real.display());
            let output = Command::new(&wrapper).arg("--version").env("PATH", &path).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "REAL|--version");
            let _ = fs::remove_dir_all(&root);
        }

        #[test]
        fn test_c_wrapper_10_remote_control_as_the_first_argument_passes_through_without_settings() {
            let root = folder("ten");
            let bin = root.join("bin");
            let real = root.join("real");
            fake_claude(&real);
            let flag_file = root.join("flag.json");
            fs::write(&flag_file, "{}").unwrap();
            fs::create_dir_all(&bin).unwrap();
            let wrapper = bin.join("claude");
            executable(&wrapper, &claude_wrapper_script(WrapperKind::Sh, flag_file.to_str().unwrap(), ""));
            let path = format!("{}:{}:/usr/bin:/bin", bin.display(), real.display());
            let server =
                Command::new(&wrapper).args(["remote-control", "--name", "my pc"]).env("PATH", &path).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&server.stdout).trim(), "REAL|remote-control --name my pc");
            // 대화 세션의 --remote-control 플래그와 프롬프트 속 단어는 하위 명령이 아니므로 --settings가 그대로 붙는다.
            let session =
                Command::new(&wrapper).args(["--remote-control", "remote-control"]).env("PATH", &path).output().unwrap();
            assert_eq!(
                String::from_utf8_lossy(&session.stdout).trim(),
                format!("REAL|--settings {} --remote-control remote-control", flag_file.display())
            );
            let _ = fs::remove_dir_all(&root);
        }

        #[test]
        fn test_c_wrapper_4_and_9_a_missing_real_claude_ends_with_a_message_even_when_the_folder_is_spelled_differently()
         {
            let root = folder("four");
            let bin = root.join("bin");
            fs::create_dir_all(&bin).unwrap();
            let wrapper = bin.join("claude");
            executable(
                &wrapper,
                &claude_wrapper_script(WrapperKind::Sh, root.join("missing.json").to_str().unwrap(), ""),
            );
            let spellings = [
                format!("{}", bin.display()),
                format!("{}/", bin.display()),
                format!("{}/../bin", bin.display()),
                format!("{}//bin", root.display()),
            ];
            for spelling in spellings {
                let output = Command::new(&wrapper)
                    .arg("--version")
                    .env("PATH", format!("{spelling}:/usr/bin:/bin"))
                    .output()
                    .unwrap();
                assert_eq!(output.status.code(), Some(127), "{spelling}: {}", String::from_utf8_lossy(&output.stderr));
                assert!(String::from_utf8_lossy(&output.stderr).contains("claude was not found in PATH"), "{spelling}");
            }
            let _ = fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn test_c_wrapper_6_the_cmd_script_drops_its_own_folder_and_adds_settings_only_when_the_flag_file_exists() {
        let script = claude_wrapper_script(
            WrapperKind::Cmd,
            "C:\\Users\\me\\.paddock\\config\\claude\\flag-settings.json",
            "C:\\Users\\me\\.paddock\\config\\bin",
        );
        for part in [
            "set \"PATH=%PATH:C:\\Users\\me\\.paddock\\config\\bin;=%\"",
            "set \"PATH=%PATH:;C:\\Users\\me\\.paddock\\config\\bin=%\"",
            "if not exist \"C:\\Users\\me\\.paddock\\config\\claude\\flag-settings.json\" goto plain",
            "\r\nif /i \"%~1\"==\"remote-control\" goto plain\r\nif not exist ",
            "\"%real%\" --settings \"C:\\Users\\me\\.paddock\\config\\claude\\flag-settings.json\" %*",
            "\r\n\"%real%\" %*\r\n",
            "if /i not \"%%~fi\"==\"%~f0\"",
            "exit /b 9009",
        ] {
            assert!(script.contains(part), "{part}");
        }
        assert_eq!(WrapperKind::Cmd.file_name(), "claude.cmd");
        assert_eq!(WrapperKind::Sh.file_name(), "claude");
    }

    #[test]
    fn test_c_wrapper_5_the_shell_word_closes_and_reopens_single_quotes() {
        assert_eq!(shell_word("/a b/it's"), "'/a b/it'\\''s'");
    }
}
