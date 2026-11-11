//! Paddock 전용 Claude 설정 쓰기: 사용량 표시·메모리 안내의 켜기/끄기를 설정 파일, 스크립트 사본, `claude` 실행 래퍼, WSL `~/.bashrc` 줄로 옮긴다.
//!
//! 쓰는 것은 모두 `usage/state.json`(마지막 설정)과 설치 경로의 순수한 결과라, 기존 앱과 같은 규칙으로 쓰면 같은 바이트가 되고 서로 덮어써도 달라지지 않는다.
//! 사용자의 `settings.json`·`.claude` 폴더는 열지 않는다. 쓰기는 모두 작업 스레드 하나가 차례로 한다(같은 파일을 동시에 고치면 한쪽 변경이 사라진다).
//! 한 작업이 패닉해도 다음 작업은 이어서 한다. 내용이 같으면 쓰지 않고, 쓸 때는 소유자만 읽는 임시 파일을 만들어 이름을 바꾼다.
//!
//! 쓰는 곳은 두 "쪽"이다. 이 컴퓨터와 WSL 배포판 하나(Windows 앱일 때). 두 쪽은 같은 함수로 같은 일을 하고, 다른 점(경로 표기·node 위치·
//! 메모리 훅을 쓸 수 있는지·파일에 닿는 길)만 `Side`가 쥔다.
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use paddock_core::account_login::WslMapping;
use paddock_core::claude_wrapper::{ClaudeWrapper, WrapperKind, claude_wrapper_script};
use paddock_core::flag_settings::{
    MemoryHook, UsageState, build_flag_settings, build_pen_mcp_config, first_line_path, flag_settings_path,
    memory_hook_command, merged_claude_state, merged_memory_state, normalize_minimum_mb, pen_mcp_config_path,
    render_json, status_line_command,
};
use paddock_core::git_paths::to_linux_argument;
use paddock_core::pen_paths::{pen_install_candidates, pencil_mcp_exe};
use paddock_core::settings_text::WslClaudeLauncherSetting;
use paddock_core::usage_model::ClaudeState;
use paddock_core::wsl_claude_launcher::{LauncherOutput, SCRIPT as LAUNCHER_SCRIPT, parse_launcher_output};
use serde_json::Value;

use super::files::{assert_plain_path, read_json};
use super::wsl;

/// 앱에 들어 있는 상태 줄 스크립트와 메모리 훅 스크립트 원본. 설정 폴더에 복사해 두면 앱을 옮기거나 새 판으로 올려도 Claude Code가 부르는 경로가 같다.
const STATUS_LINE_SCRIPT: &[u8] = include_bytes!("../../../../assets/claude-statusline.cjs");
const MEMORY_HOOK_SCRIPT: &[u8] = include_bytes!("../../../../assets/memory-hook.sh");
const STATUS_LINE_SCRIPT_NAME: &str = "claude-statusline.cjs";
const USAGE_FILE_NAME: &str = "claude.json";
const NODE_MISSING: &str = "Node.js is needed to show Claude usage. Install Node.js and try again.";
const REDIRECTED: &str =
    "A usage path is redirected or has the wrong file type. Restore the account folder before retrying.";
const UNREADABLE_RECORD: &str = "A usage record cannot be read. Check its permissions and JSON format.";

/// WSL 안에서 고정 스크립트를 실행하는 길. 일반 실행은 `wsl.exe`이고, 시험은 Linux `sh`로 바꿔 끼운다.
pub trait Shell: Send + Sync {
    /// 인자(`install <본문>`·`add`·`remove`)를 고정 스크립트에 넘겨 출력을 돌려준다.
    fn run(
        &self,
        args: &[&str],
    ) -> Result<String, String>;
}

/// 배포판 하나의 `wsl.exe`로 고정 스크립트를 실행한다.
struct WslShell {
    distribution: String,
    home: String,
}

impl Shell for WslShell {
    fn run(
        &self,
        args: &[&str],
    ) -> Result<String, String> {
        wsl::run_script(&self.distribution, &self.home, LAUNCHER_SCRIPT, args)
    }
}

/// 설정 파일이 닿는 길: 도구가 보는 경로를 이 앱이 쓸 수 있는 경로로 바꾸는 방법.
enum Reach {
    /// 이 컴퓨터: 같은 경로다.
    Direct,
    /// WSL: `\\wsl.localhost\<배포판>\…` 경로로 바꾼다.
    Mapped(WslMapping),
    /// 시험: Linux 임시 폴더 아래로 바꾼다.
    #[cfg(test)]
    Under(PathBuf),
}

/// 파일이 놓이는 한쪽: 이 컴퓨터, 또는 WSL 배포판 하나.
pub struct Side {
    /// 오류 문장에 보이는 이름.
    pub label: &'static str,
    /// 도구(Claude Code·`sh`)가 보는 설정 폴더(`~/.paddock/config`).
    pub config_dir: String,
    /// 그 경로의 구분자.
    pub separator: char,
    /// 그 쪽의 Claude Code가 Windows 프로그램인지(상태 줄 명령의 따옴표 규칙이 다르다).
    pub is_windows: bool,
    /// 메모리 안내 훅을 쓸 수 있는지. 훅이 `sh`와 `/proc/meminfo`를 쓰므로 Linux와 WSL만이다.
    pub hook_supported: bool,
    /// 그 쪽의 node 실행 파일 경로. 모르면 None이다.
    pub node: Option<String>,
    /// node가 없을 때 사용량 표시를 켜는 일을 오류로 알릴지. 이 컴퓨터는 알리고, WSL은 상태 줄만 뺀다(WSL 터미널의 Claude는 그대로 쓸 수 있다).
    pub node_required: bool,
    // -- Pen 연동 --
    /// 그 쪽의 Claude Code가 띄울 `paddock-pen-mcp` 경로. 이 컴퓨터는 이 앱 옆에 있을 때만, WSL은 그 파일의 `/mnt/…` 경로다. 없으면 None이다.
    pub pen_mcp_exe: Option<String>,
    /// 그 쪽이 띄울 Pen의 Pencil MCP 프로그램 경로(`pen_mcp_exe`와 같은 표기). Pen이 설치돼 있을 때만 있다.
    pub pencil_exe: Option<String>,
    /// WSL 쪽의 배포판 이름. `pen-open`이 `/home/…` 경로를 Windows 경로로 바꾸는 데 쓴다. 이 컴퓨터는 None이다.
    pub wsl_distribution: Option<String>,
    reach: Reach,
}

impl Side {
    /// 도구가 보는 경로를 이어 붙인다.
    fn tool_path(
        &self,
        parts: &[&str],
    ) -> String {
        let mut path = self.config_dir.clone();
        for part in parts {
            path.push(self.separator);
            path.push_str(part);
        }
        path
    }

    /// 도구가 보는 경로를 이 앱이 쓰는 경로로 바꾼다.
    fn local(
        &self,
        tool_path: &str,
    ) -> Result<PathBuf, String> {
        match &self.reach {
            Reach::Direct => Ok(PathBuf::from(tool_path)),
            Reach::Mapped(mapping) => mapping
                .to_local(tool_path)
                .map(PathBuf::from)
                .ok_or_else(|| format!("The {} folder cannot be reached.", self.label)),
            #[cfg(test)]
            Reach::Under(root) => Ok(root.join(tool_path.trim_start_matches('/'))),
        }
    }

    /// `usage` 폴더 안 파일의 이 앱 경로.
    fn usage_file(
        &self,
        name: &str,
    ) -> Result<PathBuf, String> {
        self.local(&self.tool_path(&[UsageDir::NAME, name]))
    }

    /// 플래그 설정 파일의 도구 경로와 이 앱 경로.
    fn flag_file(&self) -> (String, Result<PathBuf, String>) {
        let tool = flag_settings_path(&self.config_dir, self.separator);
        let local = self.local(&tool);
        (tool, local)
    }

    /// Pen 연동 MCP 설정 파일의 도구 경로와 이 앱 경로.
    fn pen_mcp_file(&self) -> (String, Result<PathBuf, String>) {
        let tool = pen_mcp_config_path(&self.config_dir, self.separator);
        let local = self.local(&tool);
        (tool, local)
    }
}

/// 이 앱 옆의 `paddock-pen-mcp` 실행 파일(Windows는 `.exe`). Windows·macOS 앱만 동봉하고, macOS에서는 열기(`pen_open`)만 동작한다.
/// Linux와 옆에 없는 경우(개발 빌드 등)는 None이다.
fn find_pen_mcp_exe() -> Option<String> {
    let name = format!("paddock-pen-mcp{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|folder| folder.join(name)));
    beside
        .filter(|exe| cfg!(any(windows, target_os = "macos")) && exe.is_file())
        .map(|exe| exe.to_string_lossy().into_owned())
}

/// 설치된 Pen의 Pencil MCP 프로그램(`Program Files`, 사용자 `Programs` 순). Pen이 없거나 그 프로그램이 없으면 None이다.
fn find_pencil_exe() -> Option<String> {
    let program_files = std::env::var("ProgramFiles").unwrap_or_default();
    let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let pen = pen_install_candidates(&program_files, &local_app_data)
        .into_iter()
        .map(PathBuf::from)
        .find(|exe| cfg!(windows) && exe.is_file());
    pen.and_then(|exe| exe.parent().map(|folder| pencil_mcp_exe(&folder.to_string_lossy())))
        .filter(|exe| Path::new(exe).is_file())
}

/// 설정 폴더 아래 사용량 폴더 이름.
struct UsageDir;

impl UsageDir {
    const NAME: &'static str = "usage";
}

/// WSL 쪽: 파일 길과 고정 스크립트 실행 길.
pub struct Remote {
    pub side: Side,
    pub shell: Box<dyn Shell>,
}

/// 쓰기 작업이 닿는 환경.
pub struct Environment {
    pub computer: Side,
    /// Windows 앱에서 WSL 배포판이 있고 쓰기가 허락될 때만 있다.
    pub wsl: Option<Remote>,
}

impl Environment {
    /// 지금 이 컴퓨터와 WSL을 살펴 만든다(`where node`·`wsl.exe`를 부르므로 작업 스레드에서만 부른다).
    pub fn detect() -> Self {
        let config = paddock_protocol::shared_config::config_dir();
        let pen_mcp_exe = find_pen_mcp_exe();
        let pencil_exe = find_pencil_exe();
        let computer = Side {
            label: "computer",
            config_dir: config.to_string_lossy().into_owned(),
            separator: std::path::MAIN_SEPARATOR,
            is_windows: cfg!(windows),
            hook_supported: cfg!(target_os = "linux"),
            node: find_node(),
            node_required: true,
            pen_mcp_exe: pen_mcp_exe.clone(),
            pencil_exe: pencil_exe.clone(),
            wsl_distribution: None,
            reach: Reach::Direct,
        };
        let remote = if cfg!(windows) && wsl::writes_allowed() {
            wsl::mapping_of(None).map(|(distribution, mapping)| {
                let home = mapping.linux_home.clone();
                // WSL 안의 Claude는 Windows 프로그램을 `/mnt/c/…` 경로로 띄운다(interop).
                let from_wsl = |exe: &Option<String>| exe.as_deref().map(|exe| to_linux_argument(exe, &distribution));
                Remote {
                    side: Side {
                        label: "WSL",
                        config_dir: format!("{home}/.paddock/config"),
                        separator: '/',
                        is_windows: false,
                        hook_supported: true,
                        node: wsl::node_of(&distribution, &home),
                        node_required: false,
                        pen_mcp_exe: from_wsl(&pen_mcp_exe),
                        pencil_exe: from_wsl(&pencil_exe),
                        wsl_distribution: Some(distribution.clone()),
                        reach: Reach::Mapped(mapping),
                    },
                    shell: Box::new(WslShell { distribution, home }),
                }
            })
        } else {
            None
        };
        Self { computer, wsl: remote }
    }

    /// 쓰기가 닿는 쪽들(이 컴퓨터, WSL 순서).
    fn sides(&self) -> Vec<&Side> {
        let mut sides = vec![&self.computer];
        sides.extend(self.wsl.as_ref().map(|remote| &remote.side));
        sides
    }
}

/// `where node`(Windows)·`which node`의 첫 줄. 찾지 못하면 None이다.
fn find_node() -> Option<String> {
    let mut command = Command::new(if cfg!(windows) { "where" } else { "which" });
    command.arg("node").stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    command.output().ok().and_then(|output| first_line_path(&String::from_utf8_lossy(&output.stdout)))
}

// -- 파일 쓰기 --

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// 소유자만 읽는 임시 파일(`<이름>.<번호>.tmp`)에 다 쓴 뒤 이름을 바꿔 놓는다. 내용이 같으면 쓰지 않고 false를 돌려준다.
/// 같은 이름의 링크나 폴더가 있으면 따라가 쓰지 않고 오류를 돌려준다. `executable`이면 실행 권한(0700)을 준다(유닉스).
fn write_if_changed(
    path: &Path,
    bytes: &[u8],
    executable: bool,
) -> Result<bool, String> {
    assert_plain_path(path, false).map_err(|_| REDIRECTED.to_string())?;
    let unchanged = std::fs::read(path).ok().as_deref() == Some(bytes);
    let written = if unchanged { Ok(()) } else { replace_file(path, bytes, executable) };
    written.map(|()| !unchanged)
}

/// 같은 폴더의 임시 파일에 다 쓴 뒤 이름을 바꿔 `path`를 바꾼다. 실패하면 임시 파일을 지운다.
fn replace_file(
    path: &Path,
    bytes: &[u8],
    executable: bool,
) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "The file has no folder.".to_string())?;
    create_private_dir(parent)?;
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let number = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{name}.{}.{number}.tmp", std::process::id()));
    write_private(&temporary, bytes, executable).and_then(|()| std::fs::rename(&temporary, path)).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        format!("{} could not be written: {}", name, error.kind())
    })
}

/// 폴더를 없으면 만든다(유닉스는 소유자만 접근).
fn create_private_dir(directory: &Path) -> Result<(), String> {
    #[cfg(unix)]
    let created = {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(directory)
    };
    #[cfg(not(unix))]
    let created = std::fs::create_dir_all(directory);
    created.map_err(|error| format!("A settings folder could not be created: {}", error.kind()))
}

fn write_private(
    path: &Path,
    bytes: &[u8],
    executable: bool,
) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if executable { 0o700 } else { 0o600 });
    }
    #[cfg(not(unix))]
    let _ = executable;
    options.open(path)?.write_all(bytes)
}

// -- 마지막 설정(usage/state.json) --

/// 이 컴퓨터의 `usage/state.json` 내용. 파일이 없으면 None이고, 읽을 수 없거나 JSON이 깨졌으면 오류다.
fn read_state(computer: &Side) -> Result<Option<Value>, String> {
    let file = computer.usage_file("state.json")?;
    assert_plain_path(&file, false).map_err(|_| REDIRECTED.to_string())?;
    read_json(&file).map_err(|_| UNREADABLE_RECORD.to_string())
}

fn write_state(
    computer: &Side,
    state: &Value,
) -> Result<(), String> {
    write_if_changed(&computer.usage_file("state.json")?, render_json(state).as_bytes(), false).map(|_| ())
}

// -- 한쪽에 하는 일 --

/// 상태 줄 스크립트 사본을 `usage` 폴더에 놓는다.
fn install_status_line_script(side: &Side) -> Result<(), String> {
    write_if_changed(&side.usage_file(STATUS_LINE_SCRIPT_NAME)?, STATUS_LINE_SCRIPT, false).map(|_| ())
}

/// 메모리 훅 스크립트 복사와 꺼짐 표시 파일을 맞추고 결과 상태(`on`·`off`)를 돌려준다.
/// 끄는데 켠 적이 없으면(스크립트가 없으면) 아무 파일도 만들지 않는다. 끌 때는 이미 떠 있는 세션이 스크립트를 계속 부를 수 있어 꺼짐 표시를 둔다.
fn set_memory_hook_files(
    side: &Side,
    enabled: bool,
) -> Result<&'static str, String> {
    let script = side.usage_file(MemoryHook::SCRIPT_NAME)?;
    let marker = side.usage_file(MemoryHook::OFF_MARKER)?;
    for file in [&script, &marker] {
        assert_plain_path(file, false).map_err(|_| REDIRECTED.to_string())?;
    }
    if enabled {
        write_if_changed(&script, MEMORY_HOOK_SCRIPT, false)?;
        let _ = std::fs::remove_file(&marker);
    } else if script.exists() {
        write_if_changed(&marker, b"", false)?;
    }
    Ok(if enabled { "on" } else { "off" })
}

/// 한쪽의 플래그 설정 파일을 `state`대로 쓴다. 켜진 기능만 담긴다.
fn write_flag_file(
    side: &Side,
    state: &UsageState,
) -> Result<(), String> {
    let command = (state.claude == ClaudeState::On).then(|| {
        status_line_command(
            side.node.as_deref(),
            &side.tool_path(&[UsageDir::NAME, STATUS_LINE_SCRIPT_NAME]),
            &side.tool_path(&[UsageDir::NAME, USAGE_FILE_NAME]),
            side.is_windows,
        )
    });
    let status_line = match command {
        Some(None) if side.node_required => Err(NODE_MISSING.to_string()),
        Some(command) => Ok(command),
        None => Ok(None),
    }?;
    let hook = state.memory.filter(|memory| memory.enabled && side.hook_supported).map(|memory| {
        memory_hook_command(&side.tool_path(&[UsageDir::NAME, MemoryHook::SCRIPT_NAME]), memory.minimum_mb)
    });
    let settings = build_flag_settings(status_line.as_deref(), hook.as_deref());
    let (_, local) = side.flag_file();
    write_if_changed(&local?, render_json(&settings).as_bytes(), false).map(|_| ())
}

/// 한쪽의 Pen 연동 MCP 설정 파일을 쓴다. 이 쪽에서 `paddock-pen-mcp`를 찾지 못했으면 파일을 만들지 않고, 이전에 쓴 파일이 있으면 지운다
/// (없어진 프로그램을 가리키는 설정이 Claude를 시작할 때마다 연결 실패를 만들지 않게 한다).
fn write_pen_mcp_file(side: &Side) -> Result<(), String> {
    let (_, local) = side.pen_mcp_file();
    let file = local?;
    match side.pen_mcp_exe.as_deref() {
        Some(exe) => {
            let config = build_pen_mcp_config(exe, side.wsl_distribution.as_deref(), side.pencil_exe.as_deref());
            write_if_changed(&file, render_json(&config).as_bytes(), false).map(|_| ())
        }
        None => {
            assert_plain_path(&file, false).map_err(|_| REDIRECTED.to_string())?;
            match std::fs::remove_file(&file) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    Err(format!("The Pen settings file could not be removed: {}", error.kind()))
                }
                _ => Ok(()),
            }
        }
    }
}

/// 이 컴퓨터의 `claude` 실행 래퍼를 `bin` 폴더에 놓는다(내용이 같으면 쓰지 않는다).
fn install_wrapper(computer: &Side) -> Result<(), String> {
    let kind = if computer.is_windows { WrapperKind::Cmd } else { WrapperKind::Sh };
    let directory = computer.tool_path(&[ClaudeWrapper::DIRECTORY]);
    let (flag_tool, _) = computer.flag_file();
    let (pen_tool, _) = computer.pen_mcp_file();
    let script = claude_wrapper_script(kind, &flag_tool, &pen_tool, &directory);
    let file = computer.local(&format!("{directory}{}{}", computer.separator, kind.file_name()))?;
    write_if_changed(&file, script.as_bytes(), true).map_err(|error| {
        if error == REDIRECTED {
            "The Claude launcher is not a regular file. Restore it before trying again.".to_string()
        } else {
            error
        }
    })?;
    Ok(())
}

/// WSL 안에 `claude` 실행 래퍼를 놓는다(9P 공유로는 실행 권한이 남지 않아 WSL 안의 고정 스크립트가 쓴다).
fn install_wsl_wrapper(remote: &Remote) -> Result<(), String> {
    let (flag_tool, _) = remote.side.flag_file();
    let (pen_tool, _) = remote.side.pen_mcp_file();
    let script = claude_wrapper_script(WrapperKind::Sh, &flag_tool, &pen_tool, "");
    remote.shell.run(&["install", &script]).map(|_| ())
}

// -- 두 쪽에 하는 일 --

/// 두 쪽의 Pen 연동 MCP 설정 파일을 쓴다. 쓰지 못한 쪽이 있어도 나머지는 쓰고, 첫 오류를 마지막에 돌려준다.
fn write_pen_mcp_files(env: &Environment) -> Result<(), String> {
    let mut first_error = None;
    for side in env.sides() {
        if let Err(error) = write_pen_mcp_file(side) {
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// 두 쪽의 플래그 설정 파일을 이 컴퓨터의 `state.json`대로 다시 쓴다. 쓰지 못한 쪽이 있어도 나머지는 쓰고, 첫 오류를 마지막에 돌려준다.
fn write_flag_files(env: &Environment) -> Result<(), String> {
    let state = UsageState::from_value(read_state(&env.computer)?.as_ref());
    let mut first_error = None;
    for side in env.sides() {
        if let Err(error) = write_flag_file(side, &state) {
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// 상태 줄 스크립트 사본을 이 컴퓨터에, 그리고 node를 아는 WSL에 놓는다.
fn install_status_line_scripts(env: &Environment) -> Result<(), String> {
    install_status_line_script(&env.computer)?;
    if let Some(remote) = env.wsl.as_ref().filter(|remote| remote.side.node.is_some()) {
        install_status_line_script(&remote.side)?;
    }
    Ok(())
}

/// 사용량 표시를 켜거나 끈다: 켜면 스크립트 사본을 놓고, `state.json`에 적은 뒤 플래그 설정 파일을 다시 쓴다.
pub fn set_claude(
    env: &Environment,
    enabled: bool,
) -> Result<(), String> {
    if enabled {
        install_status_line_scripts(env)?;
    }
    let current = read_state(&env.computer)?;
    write_state(
        &env.computer,
        &merged_claude_state(current.as_ref(), if enabled { ClaudeState::On } else { ClaudeState::Off }),
    )?;
    write_flag_files(env)
}

/// 메모리 안내의 결과: 적용한 쪽이 있는지.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryNoticeState {
    On,
    Off,
    /// 적용할 곳이 없다(훅이 `sh`와 `/proc/meminfo`를 쓰므로 Linux·WSL만 있다).
    Unsupported,
}

/// 메모리 안내를 켜거나 끈다: 훅 스크립트와 꺼짐 표시를 맞추고, 적용했으면 켜짐 여부·기준값을 `state.json`에 적은 뒤 플래그 설정 파일을 다시 쓴다.
pub fn set_memory_notice(
    env: &Environment,
    enabled: bool,
    minimum_mb: u32,
) -> Result<MemoryNoticeState, String> {
    let mut state = MemoryNoticeState::Unsupported;
    if env.computer.hook_supported {
        state = if set_memory_hook_files(&env.computer, enabled)? == "on" {
            MemoryNoticeState::On
        } else {
            MemoryNoticeState::Off
        };
    }
    if let Some(remote) = &env.wsl {
        // 끄는데 켠 적이 없으면(스크립트가 없으면) 앱을 시작할 때마다 WSL 폴더를 만들지 않도록 건드리지 않는다.
        let script = remote.side.usage_file(MemoryHook::SCRIPT_NAME)?;
        state = if enabled || script.exists() {
            if set_memory_hook_files(&remote.side, enabled)? == "on" {
                MemoryNoticeState::On
            } else {
                MemoryNoticeState::Off
            }
        } else {
            MemoryNoticeState::Off
        };
    }
    if state != MemoryNoticeState::Unsupported {
        let current = read_state(&env.computer)?;
        write_state(&env.computer, &merged_memory_state(current.as_ref(), enabled, f64::from(minimum_mb)))?;
    }
    write_flag_files(env)?;
    Ok(state)
}

/// WSL 일반 터미널의 PATH에 래퍼 폴더를 앞세우는 `~/.bashrc` 줄을 넣거나(`enable`) 뺀다. WSL이 없으면 `unsupported`다.
pub fn set_wsl_launcher_path(
    env: &Environment,
    enable: bool,
) -> Result<LauncherOutput, String> {
    match &env.wsl {
        Some(remote) => {
            remote.shell.run(&[if enable { "add" } else { "remove" }]).map(|output| parse_launcher_output(&output))
        }
        None => Ok(LauncherOutput { state: "unsupported".to_string(), file: String::new(), is_linked: false }),
    }
}

// -- 작업 --

/// 쓰기 작업 하나.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// 앱을 시작할 때 한 번: 지난번 설정대로 파일과 래퍼를 다시 만들고, 설정 파일의 메모리 안내·WSL 래퍼 설정을 맞춘다.
    Start {
        /// 상태 줄의 Claude usage 항목 설정(기본 켬). 마지막 설정과 다르면 맞춘다.
        claude: bool,
        /// 설정 파일의 메모리 안내(켜짐·기준 MB).
        memory: (bool, u32),
        launcher: WslClaudeLauncherSetting,
    },
    /// 사용량 표시를 켜거나 끈다. `announce`이면 처음 자동으로 켠 것이라 알린다.
    Claude {
        enabled: bool,
        announce: bool,
    },
    MemoryNotice {
        enabled: bool,
        minimum_mb: u32,
    },
    /// `~/.bashrc` 줄을 넣거나 뺀다. `quiet`이면 결과를 알리지 않는다(이미 허락한 줄을 조용히 다시 넣을 때).
    WslLauncher {
        enable: bool,
        quiet: bool,
    },
}

/// 작업 결과: 창이 상태 줄에 보일 문장들과 후속 일.
#[derive(Debug, Default, PartialEq)]
pub struct Done {
    pub notices: Vec<String>,
    /// 사용량 설정이 바뀌어 사용량을 다시 읽어야 한다.
    pub usage_changed: bool,
    /// `~/.bashrc` 줄 작업의 결과 상태(`added`·`present`·`removed`·`absent`·`blocked`·`unsupported`).
    pub launcher: Option<String>,
    /// WSL 래퍼 설정이 `ask`라서 사용자에게 한 번 물어야 한다.
    pub ask_launcher: bool,
}

/// `~/.bashrc` 작업 결과를 사용자에게 보일 문장으로 바꾼다. 알릴 일이 없으면 None이다.
///
/// Examples
/// --------
/// | 상태                     | 결과                                                                                   |
/// |--------------------------|----------------------------------------------------------------------------------------|
/// | `added`, 링크 `/repo/rc` | `/repo/rc changed. If you keep it in a repository, commit or sync it.`                 |
/// | `blocked`                | `Paddock could not change your WSL ~/.bashrc because it is a broken link or …`         |
/// | `added`, 링크 아님       | None                                                                                   |
fn launcher_notice(output: &LauncherOutput) -> Option<String> {
    if output.is_linked && (output.state == "added" || output.state == "removed") {
        Some(format!("{} changed. If you keep it in a repository, commit or sync it.", output.file))
    } else if output.state == "blocked" {
        Some(
            "Paddock could not change your WSL ~/.bashrc because it is a broken link or not a regular file."
                .to_string(),
        )
    } else {
        None
    }
}

/// 작업 하나를 실행한다.
fn run_job(
    env: &Environment,
    job: &Job,
) -> Done {
    let mut done = Done::default();
    match job {
        Job::Start { claude, memory, launcher } => start(env, *claude, *memory, *launcher, &mut done),
        Job::Claude { enabled, announce } => {
            match set_claude(env, *enabled) {
                Ok(()) if *enabled && *announce => {
                    done.notices.push("Claude usage is on. Turn it off in Quick settings.".to_string())
                }
                Ok(()) => {}
                Err(error) => done.notices.push(format!("Claude usage could not be changed: {error}")),
            }
            done.usage_changed = true;
        }
        Job::MemoryNotice { enabled, minimum_mb } => match set_memory_notice(env, *enabled, *minimum_mb) {
            Ok(MemoryNoticeState::Unsupported) => done.notices.push(
                "The memory notice needs WSL or Linux. It is not available for Windows-native Claude.".to_string(),
            ),
            Ok(_) => {}
            Err(error) => done.notices.push(format!("Memory notice: {error}")),
        },
        Job::WslLauncher { enable, quiet } => match set_wsl_launcher_path(env, *enable) {
            Ok(output) => {
                if !quiet {
                    done.notices.extend(launcher_notice(&output));
                }
                done.launcher = Some(output.state);
            }
            Err(error) if !quiet => {
                done.notices.push(format!("Paddock could not change your WSL ~/.bashrc: {error}"));
            }
            Err(_) => {}
        },
    }
    done
}

/// 시작 작업의 한 단계를 실행하고, 실패하면 `<라벨>: <이유>` 문장을 모은다.
fn attempt(
    notices: &mut Vec<String>,
    label: &str,
    step: &dyn Fn() -> Result<(), String>,
) {
    if let Err(error) = step() {
        notices.push(format!("{label}: {error}"));
    }
}

/// 시작 작업: 단계마다 따로 실패를 받는다. 앞 단계(예: node를 못 찾아 설정 파일을 못 씀)가 실패해도 래퍼 설치와 WSL 단계는 이어 하고,
/// 실패는 문장으로 모아 창이 상태 줄에 보인다. 사용자의 `settings.json`에서 옛 항목을 걷어 내는 일은 하지 않는다(기존 앱의 몫).
fn start(
    env: &Environment,
    claude: bool,
    memory: (bool, u32),
    launcher: WslClaudeLauncherSetting,
    done: &mut Done,
) {
    let mut usage_changed = false;
    let notices = &mut done.notices;
    attempt(notices, "Paddock Claude settings could not be written", &|| {
        let state = UsageState::from_value(read_state(&env.computer)?.as_ref());
        if state.claude == ClaudeState::On {
            install_status_line_scripts(env)?;
        }
        // 설정 파일의 메모리 안내가 마지막 설정과 다르면 설정 파일 쪽으로 맞춘다. 같으면 켜져 있던 안내의 스크립트를 이 앱 판으로 다시 놓는다.
        let applied = state.memory.map(|memory| (memory.enabled, memory.minimum_mb));
        let wanted = (memory.0, f64::from(memory.1));
        if applied != Some(wanted) && (memory.0 || applied.is_some_and(|applied| applied.0)) {
            set_memory_notice(env, memory.0, memory.1).map(|_| ())
        } else if state.memory_enabled() {
            let minimum = state.memory.map_or(f64::NAN, |memory| memory.minimum_mb);
            set_memory_notice(env, true, normalize_minimum_mb(minimum) as u32).map(|_| ())
        } else {
            write_flag_files(env)
        }
    });
    // 상태 줄의 Claude usage 항목과 마지막 설정이 어긋나 있으면 항목 쪽으로 맞춘다(처음 실행이면 자동으로 켜진다).
    let current = read_state(&env.computer)
        .map(|state| UsageState::from_value(state.as_ref()).claude)
        .unwrap_or(ClaudeState::Unset);
    if let Some(announce) = claude_change(claude, current) {
        match set_claude(env, claude) {
            Ok(()) if announce => notices.push("Claude usage is on. Turn it off in Quick settings.".to_string()),
            Ok(()) => {}
            Err(error) => notices.push(format!("Claude usage could not be changed: {error}")),
        }
        usage_changed = true;
    }
    // 래퍼는 Pen 설정 파일이 있을 때만 `--mcp-config`를 붙이므로, 래퍼를 놓기 전에 파일부터 맞춘다.
    attempt(notices, "The Pen settings could not be written", &|| write_pen_mcp_files(env));
    attempt(notices, "The claude launcher could not be installed", &|| install_wrapper(&env.computer));
    if let Some(remote) = &env.wsl {
        attempt(notices, "The claude launcher could not be installed in WSL", &|| install_wsl_wrapper(remote));
        match launcher {
            WslClaudeLauncherSetting::Ask => done.ask_launcher = true,
            WslClaudeLauncherSetting::On => {
                // 이미 허락한 줄이 .bashrc에 남아 있는지 확인하고 없으면 조용히 다시 넣는다.
                done.launcher = set_wsl_launcher_path(env, true).ok().map(|output| output.state);
            }
            WslClaudeLauncherSetting::Off => {}
        }
    }
    done.usage_changed |= usage_changed;
}

/// 상태 줄의 Claude usage 항목(`wants`)과 마지막 설정(`state`)이 어긋날 때 맞출 일을 정한다. 맞으면 None이고,
/// 맞출 때는 알릴지를 돌려준다: 아직 정한 적 없던 것을 처음 켤 때만 알린다.
///
/// Examples
/// --------
/// | 항목 | 마지막 설정 | 결과         |
/// |------|-------------|--------------|
/// | 켬   | Unset       | Some(true)   |
/// | 켬   | Off         | Some(false)  |
/// | 켬   | On          | None         |
/// | 끔   | On          | Some(false)  |
/// | 끔   | Unset       | Some(false)  |
/// | 끔   | Off         | None         |
fn claude_change(
    wants: bool,
    state: ClaudeState,
) -> Option<bool> {
    let differs = if wants { state != ClaudeState::On } else { state != ClaudeState::Off };
    differs.then_some(wants && state == ClaudeState::Unset)
}

/// 같은 문장을 반복해 띄우지 않으려고 마지막으로 알린 문장을 기억한다. 알릴 문장이 없는 결과(성공)가 오면 기억을 비워 같은 실패가 다시 나면 또 알린다.
#[derive(Debug, Default)]
pub struct NoticeFilter {
    last: String,
}

impl NoticeFilter {
    /// 작업 결과의 문장들 가운데 이번에 보일 문장. 첫 문장이 직전에 알린 것과 같으면 None이다.
    ///
    /// Examples
    /// --------
    /// | 직전      | 이번 문장들 | 결과                  |
    /// |-----------|-------------|-----------------------|
    /// | (없음)    | `A`         | `A`                   |
    /// | `A`       | `A`         | None                  |
    /// | `A`       | `B`         | `B`                   |
    /// | `A`       | (없음)      | None, 기억을 비움     |
    pub fn fresh(
        &mut self,
        notices: &[String],
    ) -> Option<String> {
        match notices.first() {
            Some(notice) if *notice != self.last => {
                self.last = notice.clone();
                Some(notice.clone())
            }
            Some(_) => None,
            None => {
                self.last.clear();
                None
            }
        }
    }
}

// -- 작업 줄 --

struct Queue {
    jobs: VecDeque<Job>,
    running: bool,
}

/// 쓰기 작업을 한 줄로 세워 작업 스레드 하나가 차례로 실행한다. 일이 없으면 스레드도 없다.
pub struct Setup {
    queue: Arc<Mutex<Queue>>,
    sender: Sender<Done>,
    results: Receiver<Done>,
    detect: Arc<dyn Fn() -> Environment + Send + Sync>,
}

impl Setup {
    pub fn new() -> Self {
        Self::with_environment(Arc::new(Environment::detect))
    }

    /// 환경을 만드는 방법을 정해서 만든다(시험이 임시 폴더 환경을 끼운다).
    pub fn with_environment(detect: Arc<dyn Fn() -> Environment + Send + Sync>) -> Self {
        let (sender, results) = mpsc::channel();
        Self { queue: Arc::new(Mutex::new(Queue { jobs: VecDeque::new(), running: false })), sender, results, detect }
    }

    /// 작업을 줄 끝에 세운다. 실행 중인 작업 스레드가 없으면 하나 시작한다.
    pub fn submit(
        &self,
        job: Job,
    ) {
        let start = match self.queue.lock() {
            Ok(mut queue) => {
                queue.jobs.push_back(job);
                let start = !queue.running;
                queue.running = true;
                start
            }
            Err(_) => false,
        };
        if start {
            let (queue, sender, detect) = (self.queue.clone(), self.sender.clone(), self.detect.clone());
            let spawned = std::thread::Builder::new().name("usage-setup".to_string()).spawn(move || {
                loop {
                    let next = queue.lock().ok().and_then(|mut queue| {
                        let next = queue.jobs.pop_front();
                        if next.is_none() {
                            queue.running = false;
                        }
                        next
                    });
                    let Some(job) = next else { break };
                    // 한 작업의 패닉이 다음 작업을 막지 않게 격리한다.
                    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_job(&detect(), &job)))
                        .unwrap_or_else(|_| Done {
                            notices: vec!["Paddock Claude settings could not be changed.".to_string()],
                            ..Done::default()
                        });
                    let _ = sender.send(done);
                }
            });
            if spawned.is_err()
                && let Ok(mut queue) = self.queue.lock()
            {
                queue.running = false;
                queue.jobs.clear();
            }
        }
    }

    /// 끝난 작업의 결과 하나(없으면 None).
    pub fn try_next(&self) -> Option<Done> {
        self.results.try_recv().ok()
    }

    /// 작업이 남았거나 실행 중인지(자동 점검이 끝나기를 기다릴 때 쓴다).
    pub fn is_busy(&self) -> bool {
        self.queue.lock().is_ok_and(|queue| queue.running || !queue.jobs.is_empty())
    }
}

impl Default for Setup {
    fn default() -> Self {
        Self::new()
    }
}

/// 자동 점검 상태 글 줄: 이 컴퓨터와 WSL의 쓴 결과. 파일 내용 전체 대신 요약(해시·명령·유무)만 적는다.
#[cfg(feature = "automation")]
pub fn summary_lines(launcher: Option<&str>) -> Vec<String> {
    let describe = |side: &Side, with_state: bool| {
        let read = |name: &str| side.usage_file(name).ok().and_then(|path| std::fs::read(path).ok());
        let flag_bytes = side.flag_file().1.ok().and_then(|path| std::fs::read(path).ok());
        let flag: Option<Value> = flag_bytes.as_deref().and_then(|bytes| serde_json::from_slice(bytes).ok());
        // 두 앱의 파일이 같은 바이트인지만 가리면 되므로 암호용이 아닌 짧은 해시(FNV-1a)면 충분하다.
        let hash = flag_bytes.as_deref().map_or("none".to_string(), |bytes| {
            let value = bytes
                .iter()
                .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3));
            format!("{value:016x}")
        });
        let status = flag
            .as_ref()
            .and_then(|flag| flag.pointer("/statusLine/command"))
            .and_then(Value::as_str)
            .unwrap_or("-")
            .to_string();
        let hook_command = flag
            .as_ref()
            .and_then(|flag| flag.pointer("/hooks/PreToolUse/0/hooks/0/command"))
            .and_then(Value::as_str)
            .unwrap_or("-")
            .to_string();
        let wrapper_name = if side.is_windows { "claude.cmd" } else { "claude" };
        let wrapper = side
            .local(&side.tool_path(&[ClaudeWrapper::DIRECTORY, wrapper_name]))
            .ok()
            .is_some_and(|path| path.is_file());
        let yes = |value: bool| if value { "yes" } else { "no" };
        let state_part = if with_state {
            let state = UsageState::from_value(
                side.usage_file("state.json").ok().and_then(|path| read_json(&path).ok().flatten()).as_ref(),
            );
            format!(
                "state={} memory={} limit={} ",
                match state.claude {
                    ClaudeState::On => "on",
                    ClaudeState::Off => "off",
                    ClaudeState::Unset => "unset",
                },
                state.memory.map_or("-", |memory| if memory.enabled { "on" } else { "off" }),
                state.memory.map_or("-".to_string(), |memory| memory.minimum_mb.to_string()),
            )
        } else {
            String::new()
        };
        format!(
            "{state_part}flag-bytes={hash} flag-status={status} flag-hook={hook_command} wrapper={} script={} hook-script={} hook-off={}",
            yes(wrapper),
            yes(read(STATUS_LINE_SCRIPT_NAME).as_deref() == Some(STATUS_LINE_SCRIPT)),
            yes(read(MemoryHook::SCRIPT_NAME).as_deref() == Some(MEMORY_HOOK_SCRIPT)),
            yes(read(MemoryHook::OFF_MARKER).is_some()),
        )
    };
    let computer_config = paddock_protocol::shared_config::config_dir();
    let computer = Side {
        label: "computer",
        config_dir: computer_config.to_string_lossy().into_owned(),
        separator: std::path::MAIN_SEPARATOR,
        is_windows: cfg!(windows),
        hook_supported: cfg!(target_os = "linux"),
        node: None,
        node_required: true,
        pen_mcp_exe: None,
        pencil_exe: None,
        wsl_distribution: None,
        reach: Reach::Direct,
    };
    let mut lines =
        vec![format!("usage-setup computer {} launcher={}", describe(&computer, true), launcher.unwrap_or("-"))];
    if cfg!(windows)
        && wsl::writes_allowed()
        && let Some((_, mapping)) = wsl::mapping_of(None)
    {
        let side = Side {
            label: "WSL",
            config_dir: format!("{}/.paddock/config", mapping.linux_home),
            separator: '/',
            is_windows: false,
            hook_supported: true,
            node: None,
            node_required: false,
            pen_mcp_exe: None,
            pencil_exe: None,
            wsl_distribution: None,
            reach: Reach::Mapped(mapping.clone()),
        };
        let rc = mapping
            .to_local(&format!("{}/.bashrc", mapping.linux_home))
            .and_then(|path| std::fs::read_to_string(path).ok());
        let n_lines =
            rc.map_or(0, |text| text.lines().filter(|line| line.contains(ClaudeWrapper::BASHRC_MARKER)).count());
        lines.push(format!("usage-setup-wsl wsl {} bashrc-lines={n_lines}", describe(&side, false)));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_core::flag_settings::{FlagSettings, flag_settings_path};
    use paddock_core::usage_model::quote_status_line_argument;

    /// 임시 폴더 아래에 두 쪽(이 컴퓨터, WSL)을 차린 시험 환경. WSL 쪽은 실제 `sh`로 고정 스크립트를 임시 HOME에서 실행한다.
    struct Fixture {
        root: PathBuf,
        node_computer: Option<String>,
        node_wsl: Option<String>,
        with_wsl: bool,
    }

    /// 임시 HOME에서 고정 스크립트를 Linux `sh`로 실행한다. 진짜 홈은 건드리지 않는다. Windows에는 `sh`가 없어 이 길을 쓰는 시험은 유닉스에서만 돈다
    /// (Windows의 WSL 실행은 대본 `usage-setup-wsl`이 임시 WSL 홈으로 확인한다).
    struct TestShell {
        home: PathBuf,
        n_runs: Arc<AtomicU64>,
    }

    impl Shell for TestShell {
        fn run(
            &self,
            args: &[&str],
        ) -> Result<String, String> {
            self.n_runs.fetch_add(1, Ordering::Relaxed);
            let output = Command::new("sh")
                .args(["-c", LAUNCHER_SCRIPT, "paddock-setup"])
                .args(args)
                .env("HOME", &self.home)
                .output()
                .map_err(|error| error.to_string())?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).into_owned())
            } else {
                Err("script failed".to_string())
            }
        }
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("paddock setup {name} {}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("computer")).unwrap();
            std::fs::create_dir_all(root.join("wsl/home/u")).unwrap();
            Self {
                root,
                node_computer: Some("/opt/node/bin/node".to_string()),
                node_wsl: Some("/home/u/.nvm/bin/node".to_string()),
                with_wsl: true,
            }
        }

        fn environment(&self) -> (Environment, Arc<AtomicU64>) {
            environment_at(&self.root, self.node_computer.clone(), self.node_wsl.clone(), self.with_wsl)
        }

        fn computer_file(
            &self,
            relative: &str,
        ) -> PathBuf {
            self.root.join("computer/.paddock/config").join(relative)
        }

        fn wsl_file(
            &self,
            relative: &str,
        ) -> PathBuf {
            self.root.join("wsl/home/u/.paddock/config").join(relative)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn environment_at(
        root: &Path,
        node_computer: Option<String>,
        node_wsl: Option<String>,
        with_wsl: bool,
    ) -> (Environment, Arc<AtomicU64>) {
        let computer_dir = root.join("computer").join(".paddock").join("config");
        let computer = Side {
            label: "computer",
            config_dir: computer_dir.to_string_lossy().into_owned(),
            separator: '/',
            is_windows: false,
            hook_supported: true,
            node: node_computer,
            node_required: true,
            pen_mcp_exe: None,
            pencil_exe: None,
            wsl_distribution: None,
            reach: Reach::Direct,
        };
        let n_runs = Arc::new(AtomicU64::new(0));
        let remote = with_wsl.then(|| Remote {
            side: Side {
                label: "WSL",
                config_dir: "/home/u/.paddock/config".to_string(),
                separator: '/',
                is_windows: false,
                hook_supported: true,
                node: node_wsl,
                node_required: false,
                pen_mcp_exe: None,
                pencil_exe: None,
                wsl_distribution: None,
                reach: Reach::Under(root.join("wsl")),
            },
            shell: Box::new(TestShell { home: root.join("wsl/home/u"), n_runs: n_runs.clone() }),
        });
        (Environment { computer, wsl: remote }, n_runs)
    }

    fn text(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    fn modified(path: &Path) -> std::time::SystemTime {
        std::fs::metadata(path).unwrap().modified().unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn test_c_d73_turning_usage_on_writes_the_flag_file_once_and_off_removes_the_status_line() {
        let fixture = Fixture::new("claude");
        let (env, _) = fixture.environment();
        set_claude(&env, true).unwrap();
        assert_eq!(text(&fixture.computer_file("usage/state.json")), "{\n  \"claude\": \"on\"\n}\n");
        assert_eq!(std::fs::read(fixture.computer_file("usage/claude-statusline.cjs")).unwrap(), STATUS_LINE_SCRIPT);
        let flag = text(&fixture.computer_file("claude/flag-settings.json"));
        let root = fixture.root.join("computer/.paddock/config");
        assert!(
            flag.contains(&format!(
                "\"command\": \"\\\"/opt/node/bin/node\\\" \\\"{0}/usage/claude-statusline.cjs\\\" \\\"{0}/usage/claude.json\\\"\"",
                root.display()
            )),
            "{flag}"
        );
        assert!(
            text(&fixture.wsl_file("claude/flag-settings.json"))
                .contains("\"/home/u/.nvm/bin/node\\\" \\\"/home/u/.paddock/config/usage/claude-statusline.cjs")
        );
        let written = modified(&fixture.computer_file("claude/flag-settings.json"));
        std::thread::sleep(std::time::Duration::from_millis(30));
        set_claude(&env, true).unwrap();
        assert_eq!(
            modified(&fixture.computer_file("claude/flag-settings.json")),
            written,
            "the same content is not written again"
        );
        set_claude(&env, false).unwrap();
        assert_eq!(text(&fixture.computer_file("claude/flag-settings.json")), "{}\n");
        assert_eq!(text(&fixture.wsl_file("claude/flag-settings.json")), "{}\n");
        assert!(fixture.computer_file("usage/claude-statusline.cjs").exists(), "the script copy stays");
        assert!(text(&fixture.computer_file("usage/state.json")).contains("\"off\""));
    }

    #[test]
    fn test_c_d73_without_node_the_computer_reports_it_and_wsl_just_omits_the_status_line() {
        let mut fixture = Fixture::new("node");
        fixture.node_computer = None;
        fixture.node_wsl = None;
        let (env, _) = fixture.environment();
        let error = set_claude(&env, true).unwrap_err();
        assert_eq!(error, NODE_MISSING);
        assert_eq!(text(&fixture.wsl_file("claude/flag-settings.json")), "{}\n", "the other side was still written");
        assert!(text(&fixture.computer_file("usage/state.json")).contains("\"on\""), "the choice is remembered");
    }

    #[test]
    fn test_c_d73_the_memory_hook_is_written_only_where_sh_and_proc_exist() {
        let fixture = Fixture::new("memory");
        let (mut env, _) = fixture.environment();
        env.computer.hook_supported = false;
        let state = set_memory_notice(&env, true, 4096).unwrap();
        assert_eq!(state, MemoryNoticeState::On);
        let flag = text(&fixture.wsl_file("claude/flag-settings.json"));
        assert!(
            flag.contains("PADDOCK_MEMORY_MIN_MB=4096 sh '/home/u/.paddock/config/usage/memory-hook.sh'"),
            "{flag}"
        );
        assert!(flag.contains("\"matcher\": \"Bash|Agent|Task\""));
        assert_eq!(
            text(&fixture.computer_file("claude/flag-settings.json")),
            "{}\n",
            "this computer has no hook there"
        );
        assert!(!fixture.computer_file("usage/memory-hook.sh").exists());
        assert_eq!(std::fs::read(fixture.wsl_file("usage/memory-hook.sh")).unwrap(), MEMORY_HOOK_SCRIPT);
        // 끄면 꺼짐 표시를 두고 훅 항목을 뺀다. 켠 적 없는 쪽에는 아무것도 만들지 않는다.
        set_memory_notice(&env, false, 4096).unwrap();
        assert!(fixture.wsl_file("usage/memory-hook.off").exists());
        assert!(!text(&fixture.wsl_file("claude/flag-settings.json")).contains("hooks"));
        assert!(text(&fixture.computer_file("usage/state.json")).contains("\"enabled\": false"));
    }

    #[test]
    fn test_c_d73_a_memory_notice_that_was_never_on_leaves_no_files_when_turned_off() {
        let mut fixture = Fixture::new("never");
        fixture.with_wsl = true;
        let (mut env, _) = fixture.environment();
        env.computer.hook_supported = false;
        let state = set_memory_notice(&env, false, 4096).unwrap();
        assert_eq!(state, MemoryNoticeState::Off);
        assert!(!fixture.wsl_file("usage/memory-hook.off").exists());
        assert!(!fixture.wsl_file("usage/memory-hook.sh").exists());
    }

    #[test]
    fn test_c_d73_the_memory_notice_is_unsupported_on_a_computer_without_hooks_and_without_wsl() {
        let mut fixture = Fixture::new("unsupported");
        fixture.with_wsl = false;
        let (mut env, _) = fixture.environment();
        env.computer.hook_supported = false;
        assert_eq!(set_memory_notice(&env, true, 4096).unwrap(), MemoryNoticeState::Unsupported);
        assert!(!fixture.computer_file("usage/state.json").exists(), "nothing was applied, so nothing is remembered");
    }

    #[cfg(unix)]
    #[test]
    fn test_c_d73_a_failed_flag_write_is_reported_and_does_not_skip_the_wrapper_install() {
        let mut fixture = Fixture::new("start");
        fixture.node_computer = None;
        // 마지막 설정이 사용량 켜짐이라 node가 없으면 설정 파일을 쓰지 못한다.
        std::fs::create_dir_all(fixture.computer_file("usage")).unwrap();
        std::fs::write(fixture.computer_file("usage/state.json"), "{\"claude\":\"on\"}").unwrap();
        let (env, n_runs) = fixture.environment();
        let mut done = Done::default();
        start(&env, true, (false, 4096), WslClaudeLauncherSetting::Off, &mut done);
        assert_eq!(done.notices, vec![format!("Paddock Claude settings could not be written: {NODE_MISSING}")]);
        assert!(fixture.computer_file("bin/claude").exists(), "the wrapper is still installed");
        assert!(fixture.wsl_file("bin/claude").exists(), "and so is the one in WSL");
        assert_eq!(n_runs.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_c_d73_an_existing_wrapper_with_the_same_text_is_not_rewritten_and_a_link_is_refused() {
        let fixture = Fixture::new("wrapper");
        let (env, _) = fixture.environment();
        install_wrapper(&env.computer).unwrap();
        let file = fixture.computer_file("bin/claude");
        let flag = flag_settings_path(&env.computer.config_dir, '/');
        let pen = pen_mcp_config_path(&env.computer.config_dir, '/');
        assert_eq!(text(&file), claude_wrapper_script(WrapperKind::Sh, &flag, &pen, ""));
        let written = modified(&file);
        std::thread::sleep(std::time::Duration::from_millis(30));
        install_wrapper(&env.computer).unwrap();
        assert_eq!(modified(&file), written);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o700);
            std::fs::remove_file(&file).unwrap();
            std::os::unix::fs::symlink("/bin/true", &file).unwrap();
            assert_eq!(
                install_wrapper(&env.computer).unwrap_err(),
                "The Claude launcher is not a regular file. Restore it before trying again."
            );
        }
        let names: Vec<_> =
            std::fs::read_dir(fixture.computer_file("bin")).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "no temporary file is left: {names:?}");
    }

    #[test]
    fn test_pen_setup_1_the_pen_settings_are_written_per_side_kept_when_unchanged_and_removed_without_the_program() {
        let fixture = Fixture::new("pen");
        let (mut env, _) = fixture.environment();
        env.computer.pen_mcp_exe = Some("C:\\Paddock\\paddock-pen-mcp.exe".to_string());
        env.computer.pencil_exe = Some("C:\\Program Files\\Pen\\mcp.exe".to_string());
        if let Some(remote) = env.wsl.as_mut() {
            remote.side.pen_mcp_exe = Some("/mnt/c/Paddock/paddock-pen-mcp.exe".to_string());
            remote.side.wsl_distribution = Some("Ubuntu".to_string());
        }
        write_pen_mcp_files(&env).unwrap();
        let expected = render_json(&build_pen_mcp_config(
            "C:\\Paddock\\paddock-pen-mcp.exe",
            None,
            Some("C:\\Program Files\\Pen\\mcp.exe"),
        ));
        let computer_file = fixture.computer_file("claude/pen-mcp.json");
        assert_eq!(text(&computer_file), expected);
        // WSL 쪽은 /mnt/c 경로와 배포판 인자를 받고, Pen이 없어 pencil 항목이 없다.
        let wsl = text(&fixture.wsl_file("claude/pen-mcp.json"));
        assert!(wsl.contains("\"command\": \"/mnt/c/Paddock/paddock-pen-mcp.exe\""), "{wsl}");
        assert!(wsl.contains("\"--wsl-distribution\"") && wsl.contains("\"Ubuntu\""), "{wsl}");
        assert!(!wsl.contains("pencil"), "{wsl}");
        // 같은 내용이면 다시 쓰지 않는다.
        let written = modified(&computer_file);
        std::thread::sleep(std::time::Duration::from_millis(30));
        write_pen_mcp_files(&env).unwrap();
        assert_eq!(modified(&computer_file), written);
        // 프로그램을 찾지 못하면 이전에 쓴 파일을 지우고, 없는 파일은 그대로 둔다.
        env.computer.pen_mcp_exe = None;
        write_pen_mcp_files(&env).unwrap();
        assert!(!computer_file.exists());
        assert!(fixture.wsl_file("claude/pen-mcp.json").exists());
        write_pen_mcp_files(&env).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_pen_setup_2_the_start_job_writes_the_pen_settings_before_the_wrapper_that_points_at_them() {
        let fixture = Fixture::new("pen-start");
        let (mut env, _) = fixture.environment();
        env.computer.pen_mcp_exe = Some("/p/paddock-pen-mcp.exe".to_string());
        let mut done = Done::default();
        start(&env, false, (false, 4096), WslClaudeLauncherSetting::Off, &mut done);
        assert!(done.notices.is_empty(), "{:?}", done.notices);
        assert!(text(&fixture.computer_file("claude/pen-mcp.json")).contains("/p/paddock-pen-mcp.exe"));
        let wrapper = text(&fixture.computer_file("bin/claude"));
        assert!(wrapper.contains("claude/pen-mcp.json"), "{wrapper}");
        assert!(wrapper.contains("--mcp-config="), "{wrapper}");
    }

    #[cfg(unix)]
    #[test]
    fn test_c_d73_the_start_job_applies_the_settings_memory_notice_and_replays_the_remembered_one() {
        let fixture = Fixture::new("sync");
        let (env, _) = fixture.environment();
        // 설정 파일이 메모리 안내를 켜 두었고 마지막 설정에는 없다 → 설정 쪽으로 맞춘다.
        let mut done = Done::default();
        start(&env, false, (true, 2048), WslClaudeLauncherSetting::Off, &mut done);
        assert!(done.notices.is_empty(), "{:?}", done.notices);
        assert!(text(&fixture.computer_file("usage/state.json")).contains("\"minimumMb\": 2048"));
        assert!(text(&fixture.wsl_file("claude/flag-settings.json")).contains("PADDOCK_MEMORY_MIN_MB=2048"));
        // 같으면 아무것도 새로 쓰지 않는다.
        let written = modified(&fixture.computer_file("claude/flag-settings.json"));
        std::thread::sleep(std::time::Duration::from_millis(30));
        start(&env, false, (true, 2048), WslClaudeLauncherSetting::Off, &mut Done::default());
        assert_eq!(modified(&fixture.computer_file("claude/flag-settings.json")), written);
        // 설정이 꺼져 있으면(기본) 아무 기록도 없는 곳에 아무것도 만들지 않는다.
        let fresh = Fixture::new("sync-default");
        let (env, _) = fresh.environment();
        start(&env, false, (false, 4096), WslClaudeLauncherSetting::Off, &mut Done::default());
        assert!(
            !text(&fresh.computer_file("usage/state.json")).contains("memory"),
            "nothing is remembered about the memory notice"
        );
        assert!(!fresh.computer_file("usage/memory-hook.sh").exists());
        assert_eq!(text(&fresh.computer_file("claude/flag-settings.json")), "{}\n");
    }

    #[test]
    fn test_c_d73_the_same_notice_is_shown_once_until_a_success_clears_it() {
        let mut filter = NoticeFilter::default();
        let a = vec!["A".to_string()];
        assert_eq!(filter.fresh(&a).as_deref(), Some("A"));
        assert_eq!(filter.fresh(&a), None, "the same sentence is not shown again");
        assert_eq!(filter.fresh(&["B".to_string(), "A".to_string()]).as_deref(), Some("B"));
        assert_eq!(filter.fresh(&[]), None, "a success says nothing and forgets");
        assert_eq!(filter.fresh(&a).as_deref(), Some("A"), "the same failure after a success is shown again");
    }

    #[test]
    fn test_c_d73_the_claude_usage_item_and_the_remembered_setting_are_brought_together_only_when_they_differ() {
        assert_eq!(claude_change(true, ClaudeState::Unset), Some(true), "the first run turns it on and says so");
        assert_eq!(claude_change(true, ClaudeState::Off), Some(false));
        assert_eq!(claude_change(true, ClaudeState::On), None);
        assert_eq!(claude_change(false, ClaudeState::On), Some(false));
        assert_eq!(claude_change(false, ClaudeState::Unset), Some(false));
        assert_eq!(claude_change(false, ClaudeState::Off), None);
    }

    #[cfg(unix)]
    #[test]
    fn test_c_d73_the_first_start_turns_usage_on_and_says_so_and_a_later_start_changes_nothing() {
        let fixture = Fixture::new("first");
        let (env, _) = fixture.environment();
        let mut done = Done::default();
        start(&env, true, (false, 4096), WslClaudeLauncherSetting::Off, &mut done);
        assert_eq!(done.notices, vec!["Claude usage is on. Turn it off in Quick settings.".to_string()]);
        assert!(done.usage_changed);
        assert!(text(&fixture.computer_file("usage/state.json")).contains("\"on\""));
        assert!(text(&fixture.computer_file("claude/flag-settings.json")).contains("statusLine"));
        let mut again = Done::default();
        start(&env, true, (false, 4096), WslClaudeLauncherSetting::Off, &mut again);
        assert_eq!(again, Done::default(), "nothing to say and nothing to reread");
        // 항목이 꺼져 있으면 꺼진 상태로 맞춘다(상태 줄 명령이 빠진다).
        let mut off = Done::default();
        start(&env, false, (false, 4096), WslClaudeLauncherSetting::Off, &mut off);
        assert!(off.usage_changed && off.notices.is_empty());
        assert_eq!(text(&fixture.computer_file("claude/flag-settings.json")), "{}\n");
    }

    #[cfg(unix)]
    #[test]
    fn test_c_d73_the_wsl_launcher_line_is_added_once_removed_cleanly_and_reported_when_linked() {
        let fixture = Fixture::new("launcher");
        std::fs::write(fixture.root.join("wsl/home/u/.bashrc"), "alias a=b\n").unwrap();
        let (env, _) = fixture.environment();
        assert_eq!(set_wsl_launcher_path(&env, true).unwrap().state, "added");
        assert_eq!(set_wsl_launcher_path(&env, true).unwrap().state, "present");
        assert_eq!(text(&fixture.root.join("wsl/home/u/.bashrc")).matches("# paddock-claude-wrapper").count(), 1);
        assert_eq!(set_wsl_launcher_path(&env, false).unwrap().state, "removed");
        assert_eq!(text(&fixture.root.join("wsl/home/u/.bashrc")), "alias a=b\n");
        // 링크이면 실제 파일을 고치고 링크는 두며, 사용자에게 저장소에 올리라고 알린다.
        #[cfg(unix)]
        {
            let rc = fixture.root.join("wsl/home/u/.bashrc");
            let repo = fixture.root.join("wsl/home/u/repo-bashrc");
            std::fs::rename(&rc, &repo).unwrap();
            std::os::unix::fs::symlink(&repo, &rc).unwrap();
            let output = set_wsl_launcher_path(&env, true).unwrap();
            assert!(output.is_linked && output.state == "added");
            assert_eq!(
                launcher_notice(&output),
                Some(format!("{} changed. If you keep it in a repository, commit or sync it.", output.file))
            );
            assert!(std::fs::symlink_metadata(&rc).unwrap().file_type().is_symlink());
            std::fs::remove_file(&repo).unwrap();
            let blocked = set_wsl_launcher_path(&env, true).unwrap();
            assert_eq!(blocked.state, "blocked");
            assert!(launcher_notice(&blocked).unwrap().starts_with("Paddock could not change your WSL ~/.bashrc"));
        }
        let mut without = Fixture::new("launcher-none");
        without.with_wsl = false;
        let (env, _) = without.environment();
        assert_eq!(set_wsl_launcher_path(&env, true).unwrap().state, "unsupported");
    }

    #[test]
    fn test_c_d73_requests_run_one_at_a_time_in_order_and_a_panic_does_not_stop_the_next_one() {
        let fixture = Fixture::new("queue");
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let calls = Arc::new(AtomicU64::new(0));
        let root = fixture.root.clone();
        let (calls_for_detect, order_for_detect) = (calls.clone(), order.clone());
        let setup = Setup::with_environment(Arc::new(move || {
            let call = calls_for_detect.fetch_add(1, Ordering::SeqCst);
            order_for_detect.lock().unwrap().push(if call == 0 { "first" } else { "second" });
            if call == 0 {
                std::thread::sleep(std::time::Duration::from_millis(80));
                panic!("the first job fails");
            }
            environment_at(&root, Some("/n/node".into()), None, false).0
        }));
        setup.submit(Job::Claude { enabled: true, announce: false });
        setup.submit(Job::Claude { enabled: false, announce: false });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut results = Vec::new();
        while results.len() < 2 && std::time::Instant::now() < deadline {
            results.extend(setup.try_next());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].notices, vec!["Paddock Claude settings could not be changed.".to_string()]);
        assert!(results[1].notices.is_empty(), "{:?}", results[1].notices);
        assert_eq!(*order.lock().unwrap(), vec!["first", "second"]);
        assert!(!setup.is_busy());
    }

    #[test]
    fn test_c_d73_the_quoted_status_line_keeps_a_home_folder_with_spaces_and_dollar_signs_literal() {
        let command = status_line_command(
            Some("/h/$x/node"),
            "/h/a b/usage/claude-statusline.cjs",
            "/h/a b/usage/claude.json",
            false,
        )
        .unwrap();
        assert_eq!(command, "\"/h/\\$x/node\" \"/h/a b/usage/claude-statusline.cjs\" \"/h/a b/usage/claude.json\"");
        assert_eq!(quote_status_line_argument("C:\\a b", true), "\"C:\\a b\"");
        assert_eq!(FlagSettings::FILE, "flag-settings.json");
    }

    #[test]
    fn test_c_d73_the_script_assets_are_byte_identical_to_the_electron_scripts() {
        let shell = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../desktop/paddock-shell");
        for (name, asset) in
            [(STATUS_LINE_SCRIPT_NAME, STATUS_LINE_SCRIPT), (MemoryHook::SCRIPT_NAME, MEMORY_HOOK_SCRIPT)]
        {
            // 기존 앱 저장소 옆에서만 대조한다(이 앱만 따로 내려받은 경우에는 건너뛴다).
            if let Ok(original) = std::fs::read(shell.join(name)) {
                assert_eq!(original, asset, "{name} differs from the Electron script");
            }
        }
    }

    #[test]
    fn test_c_d73_a_redirected_usage_folder_is_refused_without_writing_through_it() {
        #[cfg(unix)]
        {
            let fixture = Fixture::new("redirect");
            let (env, _) = fixture.environment();
            let elsewhere = fixture.root.join("elsewhere");
            std::fs::create_dir_all(&elsewhere).unwrap();
            std::fs::create_dir_all(fixture.computer_file("usage")).unwrap();
            std::os::unix::fs::symlink(&elsewhere, fixture.computer_file("usage/state.json")).unwrap();
            assert_eq!(set_claude(&env, false).unwrap_err(), REDIRECTED);
            assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
        }
    }

    #[test]
    fn test_c_d73_a_broken_state_file_is_reported_and_left_alone() {
        let fixture = Fixture::new("broken");
        std::fs::create_dir_all(fixture.computer_file("usage")).unwrap();
        std::fs::write(fixture.computer_file("usage/state.json"), "not json").unwrap();
        let (env, _) = fixture.environment();
        assert_eq!(set_claude(&env, false).unwrap_err(), UNREADABLE_RECORD);
        assert_eq!(text(&fixture.computer_file("usage/state.json")), "not json");
    }
}
