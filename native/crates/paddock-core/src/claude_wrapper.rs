//! 일반 터미널에서 친 `claude`에 Paddock 전용 설정(`--settings <플래그 파일>`)이 붙게 하는 실행 래퍼의 본문.
//!
//! 계정 터미널은 시작 명령에 `--settings`를 직접 넣지만, 일반 터미널은 사용자가 `claude`를 직접 치므로
//! 설정 폴더의 `bin` 폴더를 터미널 PATH 맨 앞에 두고(`claude_wrapper_path`) 거기에 같은 이름의 래퍼를 놓는다.
//! 래퍼는 자기 폴더를 PATH에서 뺀 뒤 진짜 `claude`를 `--settings`와 함께 실행하고, 나머지 인자는 그대로 넘긴다.
//! 플래그 파일이 없거나 첫 인자가 `remote-control`이면 인자를 붙이지 않는다. 기존 앱과 같은 글자여야 두 앱이 서로 덮어쓰지 않으므로 본문은 글자 그대로 옮겼다(Pen 연동의 `--mcp-config` 부분과
//! cmd 래퍼가 경로를 자기 위치(`%~dp0`)에서 펼치는 부분은 두 앱을 함께 고쳤다).
//!
//! Pen 연동 MCP 설정 파일(`--mcp-config`)도 같은 규칙으로 붙인다: 파일이 있을 때만, `remote-control`이 아닐 때만. `--mcp-config`는 값을 여러 개 받는 옵션이라
//! `--mcp-config <파일>`로 쓰면 뒤따르는 프롬프트 인자(`claude "문장"`)까지 설정 파일로 읽는다. 그래서 `--mcp-config=<파일>` 한 단어로 붙인다.

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

/// cmd 래퍼가 파일을 가리키는 글자. 래퍼 폴더(`own_directory`)의 부모 폴더 아래 파일이면 래퍼 위치에서 펼치는 `%~dp0..\<나머지>`이고,
/// 아니면 경로 그대로다.
///
/// 왜: cmd는 배치 파일을 콘솔 코드 페이지(한국어 Windows는 949)로 읽어, UTF-8로 적어 둔 한글 경로가 깨지고 `if exist`가 거짓이 된다.
/// `%~dp0`(이 배치 파일의 폴더)은 실행 중에 펼쳐지므로 코드 페이지와 상관없이 맞다.
///
/// Examples
/// --------
/// | file                              | own_directory             | 결과                       |
/// |-----------------------------------|---------------------------|----------------------------|
/// | `C:\Users\영재\config\claude\f.json` | `C:\Users\영재\config\bin` | `%~dp0..\claude\f.json`    |
/// | `D:\other\f.json`                 | `C:\config\bin`           | `D:\other\f.json`          |
fn cmd_reference(
    file: &str,
    own_directory: &str,
) -> String {
    let parent = own_directory.trim_end_matches('\\').rsplit_once('\\').map(|(parent, _)| parent);
    match parent.and_then(|parent| file.strip_prefix(parent)).and_then(|rest| rest.strip_prefix('\\')) {
        Some(rest) => format!("%~dp0..\\{rest}"),
        None => file.to_string(),
    }
}

/// 래퍼 스크립트 본문.
///
/// `own_directory`는 래퍼 폴더의 경로다(cmd 래퍼가 두 설정 파일을 자기 위치 기준으로 가리킬 때 쓴다, `cmd_reference`. sh 래퍼는 쓰지 않는다).
/// `mcp_config_file`은 Pen 연동 MCP 설정 파일이고, 있을 때만 `--mcp-config=<파일>`로 붙는다.
///
/// Examples
/// --------
/// | kind | 하는 일                                                                                                  |
/// |------|----------------------------------------------------------------------------------------------------------|
/// | Sh   | 자기 폴더를 PATH에서 뺌 → 플래그 파일이 있으면 `--settings <파일>`, Pen 설정 파일이 있으면 `--mcp-config=<파일>`을 앞에 더해 `exec claude … "$@"` |
/// | Cmd  | 자기 폴더(`%~dp0`)를 PATH에서 뺌 → 같은 규칙으로 `claude --settings "%~dp0..\<파일>" --mcp-config="%~dp0..\<파일>" %*` |
/// | 둘 다 | `claude remote-control ...`은 두 파일이 있어도 인자를 더하지 않고 그대로 넘긴다                                  |
pub fn claude_wrapper_script(
    kind: WrapperKind,
    flag_file: &str,
    mcp_config_file: &str,
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
            &format!("mcp={}", shell_word(mcp_config_file)),
            // `claude remote-control`(원격 접속 서버)은 하위 명령 앞의 `--settings`를 자기가 띄우는 세션에 넘길 수 없다며 시작을 거부한다.
            // 그래서 이 명령일 때만 플래그 파일을 붙이지 않는다. 대화 세션의 `--remote-control`·`/remote-control`은 `--settings`와 함께 동작한다.
            // Pen 설정 파일도 같은 규칙이다. `--mcp-config=` 한 단어로 붙여, 값을 여러 개 받는 옵션이 뒤의 프롬프트 인자를 삼키지 않게 한다.
            "if [ \"$1\" != remote-control ]; then",
            "    if [ -f \"$mcp\" ]; then set -- \"--mcp-config=$mcp\" \"$@\"; fi",
            "    if [ -f \"$flag\" ]; then set -- --settings \"$flag\" \"$@\"; fi",
            "fi",
            "exec \"$real\" \"$@\"",
            "",
        ]
        .join("\n"),
        WrapperKind::Cmd => {
            let flag_file = cmd_reference(flag_file, own_directory);
            let mcp_config_file = cmd_reference(mcp_config_file, own_directory);
            [
            "@echo off",
            "rem Paddock가 만든 claude 실행 래퍼. Paddock 전용 설정(상태 줄·메모리 안내)을 --settings로 덧붙이고 진짜 claude를 실행한다.",
            "setlocal",
            // 자기 폴더와 설정 파일은 글자로 적지 않고 이 파일의 위치(`%~dp0`)에서 펼친다. 적어 둔 한글 경로는 콘솔 코드 페이지에 따라 깨진다.
            // 펼친 값으로 PATH를 고치려면 한 번 더 펼쳐야 해서 `call set`을 쓴다.
            "set \"own=%~dp0\"",
            "set \"own=%own:~0,-1%\"",
            "call set \"PATH=%%PATH:%own%;=%%\"",
            "call set \"PATH=%%PATH:;%own%=%%\"",
            // 위에서 못 뺀 다른 표기의 자기 폴더가 PATH에 남아도 자기 자신을 다시 실행하지 않도록, 찾은 claude가 이 파일이면 멈춘다.
            "set \"real=\"",
            "for /f \"delims=\" %%i in ('where claude 2^>nul') do if not defined real if /i not \"%%~fi\"==\"%~f0\" set \"real=%%i\"",
            "if not defined real (",
            "    echo claude was not found in PATH ^(Paddock launcher found only itself^). Install Claude Code, then try again. 1>&2",
            "    exit /b 9009",
            ")",
            // sh 래퍼와 같은 이유로 `claude remote-control`에는 --settings·--mcp-config를 붙이지 않는다.
            // 파일 유무 네 경우를 변수에 모으지 않고 각각 한 줄로 펼친다. 경로를 변수에 담으면 `&`·`|`·`<`·`>`가 든 경로에서 명령이 깨진다.
            "if /i \"%~1\"==\"remote-control\" goto plain",
            &format!("if exist \"{flag_file}\" goto flag"),
            &format!("if exist \"{mcp_config_file}\" goto mcp"),
            "goto plain",
            ":flag",
            &format!("if exist \"{mcp_config_file}\" goto both"),
            &format!("\"%real%\" --settings \"{flag_file}\" %*"),
            "exit /b %ERRORLEVEL%",
            ":both",
            &format!("\"%real%\" --settings \"{flag_file}\" --mcp-config=\"{mcp_config_file}\" %*"),
            "exit /b %ERRORLEVEL%",
            ":mcp",
            &format!("\"%real%\" --mcp-config=\"{mcp_config_file}\" %*"),
            "exit /b %ERRORLEVEL%",
            ":plain",
            "\"%real%\" %*",
            "exit /b %ERRORLEVEL%",
            "",
            ]
            .join("\r\n")
        }
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
        let script = claude_wrapper_script(WrapperKind::Sh, "/tmp/it's flag.json", "/no/such/pen-mcp.json", "");
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

        /// 명령을 실행해 출력을 받는다. 방금 쓴 실행 파일은, 다른 시험 스레드가 그 사이 띄운 자식 프로세스가 쓰기 핸들을 잠깐 물려받으면
        /// 실행이 `ExecutableFileBusy`(ETXTBSY)로 실패한다. 그때만 잠깐 쉬고 다시 실행한다.
        fn output_of(command: &mut Command) -> std::process::Output {
            const MAX_TRIES: u32 = 50;
            let mut n_tries = 1;
            loop {
                match command.output() {
                    Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy && n_tries < MAX_TRIES => {
                        n_tries += 1;
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    result => break result.unwrap(),
                }
            }
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
            executable(
                &wrapper,
                &claude_wrapper_script(WrapperKind::Sh, flag_file.to_str().unwrap(), "/no/such/pen-mcp.json", ""),
            );
            let path = format!("{}:{}:/usr/bin:/bin", bin.display(), real.display());
            let output = output_of(Command::new(&wrapper).args(["-p", "two words", "--model", "x"]).env("PATH", &path));
            assert_eq!(output.status.code(), Some(7));
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                format!("REAL|--settings {} -p two words --model x", flag_file.display())
            );
            let _ = fs::remove_dir_all(&root);
        }

        #[test]
        fn test_pen_wrapper_1_the_pen_mcp_file_is_added_as_one_word_after_settings_and_never_for_remote_control() {
            let root = folder("pen1");
            let bin = root.join("bin");
            let real = root.join("real");
            fake_claude(&real);
            let flag_file = root.join("flag.json");
            let mcp_file = root.join("pen mcp.json");
            fs::write(&flag_file, "{}").unwrap();
            fs::write(&mcp_file, "{}").unwrap();
            fs::create_dir_all(&bin).unwrap();
            let wrapper = bin.join("claude");
            executable(
                &wrapper,
                &claude_wrapper_script(WrapperKind::Sh, flag_file.to_str().unwrap(), mcp_file.to_str().unwrap(), ""),
            );
            let path = format!("{}:{}:/usr/bin:/bin", bin.display(), real.display());
            let run = |args: &[&str]| {
                let output = output_of(Command::new(&wrapper).args(args).env("PATH", &path));
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            };
            // 두 파일이 있으면 --settings 다음에 --mcp-config=<파일>이 오고, 프롬프트 인자는 그 뒤에 그대로 온다.
            assert_eq!(
                run(&["fix it"]),
                format!("REAL|--settings {} --mcp-config={} fix it", flag_file.display(), mcp_file.display())
            );
            assert_eq!(
                run(&[]),
                format!("REAL|--settings {} --mcp-config={}", flag_file.display(), mcp_file.display())
            );
            // remote-control은 어느 파일도 붙이지 않는다.
            assert_eq!(run(&["remote-control", "--name", "x"]), "REAL|remote-control --name x");
            // 플래그 파일만 없으면 Pen 설정만 붙는다.
            fs::remove_file(&flag_file).unwrap();
            assert_eq!(run(&["-p", "q"]), format!("REAL|--mcp-config={} -p q", mcp_file.display()));
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
                &claude_wrapper_script(
                    WrapperKind::Sh,
                    root.join("missing.json").to_str().unwrap(),
                    "/no/such/pen-mcp.json",
                    "",
                ),
            );
            let path = format!("{}:{}:/usr/bin:/bin", bin.display(), real.display());
            let output = output_of(Command::new(&wrapper).arg("--version").env("PATH", &path));
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
            executable(
                &wrapper,
                &claude_wrapper_script(WrapperKind::Sh, flag_file.to_str().unwrap(), "/no/such/pen-mcp.json", ""),
            );
            let path = format!("{}:{}:/usr/bin:/bin", bin.display(), real.display());
            let server =
                output_of(Command::new(&wrapper).args(["remote-control", "--name", "my pc"]).env("PATH", &path));
            assert_eq!(String::from_utf8_lossy(&server.stdout).trim(), "REAL|remote-control --name my pc");
            // 대화 세션의 --remote-control 플래그와 프롬프트 속 단어는 하위 명령이 아니므로 --settings가 그대로 붙는다.
            let session =
                output_of(Command::new(&wrapper).args(["--remote-control", "remote-control"]).env("PATH", &path));
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
                &claude_wrapper_script(
                    WrapperKind::Sh,
                    root.join("missing.json").to_str().unwrap(),
                    "/no/such/pen-mcp.json",
                    "",
                ),
            );
            let spellings = [
                format!("{}", bin.display()),
                format!("{}/", bin.display()),
                format!("{}/../bin", bin.display()),
                format!("{}//bin", root.display()),
            ];
            for spelling in spellings {
                let output =
                    output_of(Command::new(&wrapper).arg("--version").env("PATH", format!("{spelling}:/usr/bin:/bin")));
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
            "C:\\Users\\me\\.paddock\\config\\claude\\pen-mcp.json",
            "C:\\Users\\me\\.paddock\\config\\bin",
        );
        for part in [
            "\r\nset \"own=%~dp0\"\r\nset \"own=%own:~0,-1%\"\r\n",
            "call set \"PATH=%%PATH:%own%;=%%\"",
            "call set \"PATH=%%PATH:;%own%=%%\"",
            "\r\nif /i \"%~1\"==\"remote-control\" goto plain\r\nif exist \"%~dp0..\\claude\\flag-settings.json\" goto flag\r\n",
            "if exist \"%~dp0..\\claude\\pen-mcp.json\" goto mcp\r\ngoto plain\r\n:flag\r\n",
            "\"%real%\" --settings \"%~dp0..\\claude\\flag-settings.json\" %*\r\n",
            "\"%real%\" --settings \"%~dp0..\\claude\\flag-settings.json\" --mcp-config=\"%~dp0..\\claude\\pen-mcp.json\" %*\r\n",
            "\"%real%\" --mcp-config=\"%~dp0..\\claude\\pen-mcp.json\" %*\r\n",
            "\r\n:plain\r\n\"%real%\" %*\r\n",
            "if /i not \"%%~fi\"==\"%~f0\"",
            "exit /b 9009",
        ] {
            assert!(script.contains(part), "{part}");
        }
        // 설정 폴더의 경로(사용자 이름 등)는 래퍼에 글자로 남지 않는다.
        assert!(!script.contains("C:\\Users\\me"), "{script}");
        // 래퍼 폴더의 부모 밖에 있는 파일은 경로 그대로 둔다. 경로를 변수에 담지 않으므로 `&`·`|`·`<`·`>`가 든 경로도 따옴표 안에만 놓인다.
        let special = claude_wrapper_script(WrapperKind::Cmd, "C:\\a&b\\f.json", "C:\\a|c\\p<q>.json", "D:\\x\\bin");
        assert!(!special.contains("set \"extra"), "{special}");
        assert!(special.contains("--settings \"C:\\a&b\\f.json\" --mcp-config=\"C:\\a|c\\p<q>.json\" %*"), "{special}");
        assert_eq!(WrapperKind::Cmd.file_name(), "claude.cmd");
        assert_eq!(WrapperKind::Sh.file_name(), "claude");
    }

    #[test]
    fn test_c_wrapper_5_the_shell_word_closes_and_reopens_single_quotes() {
        assert_eq!(shell_word("/a b/it's"), "'/a b/it'\\''s'");
    }
}
