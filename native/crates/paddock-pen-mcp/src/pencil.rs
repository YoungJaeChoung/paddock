//! Pen 앱 쪽: 설치 위치 찾기, Pencil MCP 프로그램을 자식 프로세스로 불러 활성 문서·문서 확인 조각을 얻기, 파일을 Pen에 넘기기.
//!
//! Pen은 파일을 여는 도구를 내놓지 않아 열기는 `Pen.exe <파일>`로 직접 한다. 활성 문서는 Pencil 도구 `get_app_state`만 알려 준다.
//! Pencil 도구 구성은 Pen 판마다 다르다(저장 도구 `save`의 유무, `execute`가 문서를 `filePath`로 받는지 `uri`로 받는지). 그래서 도구 목록을
//! 읽어 두고 그 구성에 맞춰 부르며, Pencil MCP 프로그램 파일이 바뀌면(Pen 업그레이드) 다시 읽는다.
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use paddock_core::pen_paths::{PenOpenFailure, pen_install_candidates, pen_open_failure, pencil_mcp_exe};
use serde_json::{Value, json};

use crate::limits::Limits;

/// 이 프로세스가 읽어 둔 Pencil 도구 구성. 목록을 못 읽은 동안은 비어 있고, 다음 호출이 다시 읽는다.
static TOOLS: Mutex<Option<CachedTools>> = Mutex::new(None);

/// 프로그램 파일의 수정 시각과 크기. 둘 중 하나라도 바뀌면 다른 판으로 본다.
type ExeStamp = (SystemTime, u64);

/// 읽어 둔 도구 구성과 그것을 내놓은 Pencil MCP 프로그램. 프로그램 위치나 파일이 바뀌면 이 구성은 더는 맞지 않는다.
struct CachedTools {
    exe: PathBuf,
    stamp: Option<ExeStamp>,
    tools: PencilTools,
}

/// 도구 호출이 끝난 모양.
#[derive(Debug, PartialEq, Eq)]
pub enum ToolOutcome {
    /// 오류 없이 답했다(글자 내용이 비어도).
    Succeeded,
    /// 오류로 답했다(도구가 준 사유 문장).
    Failed(String),
    /// 기한 안에 답이 없었다(프로그램을 띄우지 못한 경우 포함).
    NoAnswer,
}

/// Pencil 도구 이름.
pub struct PencilTool;

impl PencilTool {
    pub const EXECUTE: &'static str = "execute";
    pub const GET_APP_STATE: &'static str = "get_app_state";
    pub const SAVE: &'static str = "save";
}

/// `execute`가 대상 문서를 받는 인자. Pen 1.2.14는 `filePath`(Windows 경로)를, 저장 도구가 생긴 판은 `uri`(`file://` 주소)를 요구한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentArgument {
    FilePath,
    Uri,
}

impl DocumentArgument {
    /// 인자 이름.
    pub fn key(self) -> &'static str {
        match self {
            Self::FilePath => "filePath",
            Self::Uri => "uri",
        }
    }

    /// Windows 경로를 이 인자의 값으로 바꾼다.
    ///
    /// Examples
    /// --------
    /// | 인자       | Windows 경로   | 결과                   |
    /// |------------|----------------|------------------------|
    /// | `FilePath` | `C:\w\a.pen`   | `C:\w\a.pen`           |
    /// | `Uri`      | `C:\w\a.pen`   | `file:///C:/w/a.pen`   |
    pub fn value(
        self,
        windows_path: &str,
    ) -> String {
        match self {
            Self::FilePath => windows_path.to_string(),
            Self::Uri => to_file_uri(windows_path),
        }
    }
}

/// Pencil MCP 프로그램이 내놓은 도구 구성.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PencilTools {
    names: Vec<String>,
    pub document_argument: DocumentArgument,
}

impl Default for PencilTools {
    /// 목록을 못 읽었을 때의 가정: 저장 도구가 없고 `execute`는 `filePath`를 받는다(Pen 1.2.14의 구성).
    fn default() -> Self {
        Self { names: Vec::new(), document_argument: DocumentArgument::FilePath }
    }
}

impl PencilTools {
    /// `tools/list` 결과(`{"tools":[…]}`)에서 도구 이름과 `execute`의 문서 인자를 읽는다.
    ///
    /// `execute`의 필수 인자에 `uri`가 있고 `filePath`가 없을 때만 `Uri`이고, 그 밖(필수 인자를 못 읽은 경우 포함)은 `FilePath`다.
    ///
    /// Examples
    /// --------
    /// | `tools` 목록                                                 | 이름               | 문서 인자  |
    /// |--------------------------------------------------------------|--------------------|------------|
    /// | `execute{required:[filePath,input]}`, `get_app_state`        | execute, get_app_state | `FilePath` |
    /// | `execute{required:[uri,input]}`, `save{required:[uri]}`      | execute, save      | `Uri`      |
    /// | (`tools` 없음)                                               | (없음)             | `FilePath` |
    pub fn from_list(list: &Value) -> Self {
        let tools = list.get("tools").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        let names =
            tools.iter().filter_map(|tool| tool.get("name").and_then(Value::as_str)).map(str::to_string).collect();
        let execute_required: Vec<&str> = tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some(PencilTool::EXECUTE))
            .and_then(|tool| tool.pointer("/inputSchema/required"))
            .and_then(Value::as_array)
            .map(|required| required.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let document_argument = if execute_required.contains(&"uri") && !execute_required.contains(&"filePath") {
            DocumentArgument::Uri
        } else {
            DocumentArgument::FilePath
        };
        Self { names, document_argument }
    }

    /// 이 이름의 도구가 있는지.
    pub fn has(
        &self,
        name: &str,
    ) -> bool {
        self.names.iter().any(|candidate| candidate == name)
    }
}

/// Windows 경로를 `file://` 주소로 바꾼다. UNC 경로의 서버 이름은 주소의 호스트가 되고, 경로 조각의 공백·`#`·`%`·한글 같은 글자는
/// UTF-8 바이트별 `%XX`로 적는다.
///
/// Examples
/// --------
/// | Windows 경로                          | 결과                                          |
/// |---------------------------------------|-----------------------------------------------|
/// | `C:\w\a.pen`                          | `file:///C:/w/a.pen`                          |
/// | `C:\w\a b#1.pen`                      | `file:///C:/w/a%20b%231.pen`                  |
/// | `\\wsl.localhost\Ubuntu\home\u\a.pen` | `file://wsl.localhost/Ubuntu/home/u/a.pen`    |
/// | `C:\w\한.pen`                         | `file:///C:/w/%ED%95%9C.pen`                  |
/// | `/w/a.pen` (Windows 밖의 절대 경로)   | `file:///w/a.pen`                             |
pub fn to_file_uri(windows_path: &str) -> String {
    let slashed = windows_path.replace('\\', "/");
    let (host, path) = match slashed.strip_prefix("//") {
        Some(unc) => {
            let (host, rest) = unc.split_once('/').unwrap_or((unc, ""));
            (host.to_string(), format!("/{rest}"))
        }
        None if slashed.starts_with('/') => (String::new(), slashed),
        None => (String::new(), format!("/{slashed}")),
    };
    // 주소의 경로 조각에 그대로 둘 수 있는 글자(영숫자·`-._~`·`!$&'()*+,;=:@`·`/`)만 남기고 나머지는 바이트별로 적는다.
    let encoded: String = path
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    format!("file://{host}{encoded}")
}

/// Pencil MCP 프로그램. 한 번 부를 때마다 프로세스를 띄우고 답을 받으면 끝낸다.
pub struct Pencil {
    exe: PathBuf,
}

impl Pencil {
    pub fn new(exe: PathBuf) -> Self {
        Self { exe }
    }

    /// Pencil 도구를 한 번 불러 글자 결과를 돌려준다. 프로그램을 띄우지 못하거나 오류이거나 `PENCIL_CALL_DEADLINE` 안에 답이 없으면 None이다.
    ///
    /// 호출은 MCP 순서(initialize → initialized 알림 → tools/call)를 그대로 밟고, 답을 받으면 프로세스를 끝낸다.
    pub fn call(
        &self,
        name: &str,
        arguments: &Value,
    ) -> Option<String> {
        self.request("tools/call", &json!({ "name": name, "arguments": arguments }), Limits::PENCIL_CALL_DEADLINE)
            .and_then(|reply| result_text(&reply))
    }

    /// Pencil 도구를 한 번 불러, 글자 내용이 아니라 성패만 돌려준다. `deadline` 안에 답이 없으면 `NoAnswer`다.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn call_for_outcome(
        &self,
        name: &str,
        arguments: &Value,
        deadline: Duration,
    ) -> ToolOutcome {
        tool_outcome(self.request("tools/call", &json!({ "name": name, "arguments": arguments }), deadline).as_ref())
    }

    /// 도구 목록을 읽는다. 프로그램을 띄우지 못하거나 오류이거나 기한 안에 답이 없으면 None이다.
    pub fn list_tools(&self) -> Option<PencilTools> {
        self.request("tools/list", &json!({}), Limits::PENCIL_CALL_DEADLINE)
            .filter(|reply| reply.get("error").is_none_or(Value::is_null))
            .and_then(|reply| reply.get("result").map(PencilTools::from_list))
    }

    /// 프로그램 파일의 수정 시각과 크기. 읽지 못하면 None이다.
    fn stamp(&self) -> Option<ExeStamp> {
        std::fs::metadata(&self.exe)
            .ok()
            .and_then(|metadata| metadata.modified().ok().map(|modified| (modified, metadata.len())))
    }

    /// 프로세스를 띄워 요청 하나를 보내고 그 응답 줄 전체를 돌려준 뒤 프로세스를 끝낸다. `deadline` 안에 답이 없으면 None이다.
    fn request(
        &self,
        method: &str,
        params: &Value,
        deadline: Duration,
    ) -> Option<Value> {
        let mut command = Command::new(&self.exe);
        command
            .args(["--app", "desktop", "--agent", "claudeCodeCLI"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().ok()?;
        let reply = Self::converse(&mut child, method, params, deadline);
        let _ = child.kill();
        let _ = child.wait();
        reply
    }

    /// 띄운 프로세스와 JSON-RPC 줄을 주고받아 요청(`id` 2)의 응답을 얻는다.
    fn converse(
        child: &mut Child,
        method: &str,
        params: &Value,
        deadline: Duration,
    ) -> Option<Value> {
        let mut stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        let (sender, lines) = mpsc::channel::<String>();
        // 읽기는 별도 스레드에서 한다. 프로세스가 답 없이 멈춰도 기한 안에 이 함수가 끝나게 하려는 것이다.
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut send = |message: Value| writeln!(stdin, "{message}").and_then(|()| stdin.flush()).is_ok();
        let mut answer = None;
        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "pen-open", "version": env!("CARGO_PKG_VERSION") }
            }
        });
        if send(initialize) {
            let deadline = Instant::now() + deadline;
            while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                let Ok(line) = lines.recv_timeout(left) else { break };
                let Ok(reply) = serde_json::from_str::<Value>(&line) else { continue };
                match reply.get("id").and_then(Value::as_i64) {
                    Some(1) => {
                        let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
                        let request = json!({ "jsonrpc": "2.0", "id": 2, "method": method, "params": params });
                        if !(send(initialized) && send(request)) {
                            break;
                        }
                    }
                    Some(2) => {
                        answer = Some(reply);
                        break;
                    }
                    _ => {}
                }
            }
        }
        answer
    }
}

/// 도구 호출 응답에서 첫 글자 내용을 꺼낸다. 오류 응답이거나 `isError` 결과면 None이다.
fn result_text(reply: &Value) -> Option<String> {
    let failed = reply.get("error").is_some_and(|error| !error.is_null())
        || reply.pointer("/result/isError").and_then(Value::as_bool) == Some(true);
    if failed { None } else { reply.pointer("/result/content/0/text").and_then(Value::as_str).map(str::to_string) }
}

/// 도구 호출 응답(없으면 None)을 성패로 가른다. 오류 응답과 `isError` 결과만 실패이고, 글자 내용이 없는 결과는 성공이다.
///
/// Examples
/// --------
/// | 응답                                                    | 결과                       |
/// |---------------------------------------------------------|----------------------------|
/// | `{result:{content:[{text:"ok"}]}}`                      | `Succeeded`                |
/// | `{result:{content:[]}}`                                 | `Succeeded`                |
/// | `{error:{message:"bad uri"}}`                           | `Failed("bad uri")`        |
/// | `{result:{isError:true, content:[{text:"no file"}]}}`   | `Failed("no file")`        |
/// | (없음)                                                  | `NoAnswer`                 |
fn tool_outcome(reply: Option<&Value>) -> ToolOutcome {
    match reply {
        None => ToolOutcome::NoAnswer,
        Some(reply) if reply.get("error").is_some_and(|error| !error.is_null()) => ToolOutcome::Failed(
            reply.pointer("/error/message").and_then(Value::as_str).unwrap_or("unknown error").to_string(),
        ),
        Some(reply) if reply.pointer("/result/isError").and_then(Value::as_bool) == Some(true) => ToolOutcome::Failed(
            reply.pointer("/result/content/0/text").and_then(Value::as_str).unwrap_or("unknown error").to_string(),
        ),
        Some(_) => ToolOutcome::Succeeded,
    }
}

/// `get_app_state` 결과에서 활성 문서 표기를 꺼낸다(`active canvas editor: \`<표기>\``). 열린 문서가 없으면 None이다.
///
/// Examples
/// --------
/// | 결과 글                                         | 결과              |
/// |-------------------------------------------------|-------------------|
/// | `... active canvas editor: \`/Ubuntu/h/a.pen\`` | `/Ubuntu/h/a.pen` |
/// | `no editor open`                                | None              |
pub fn parse_active_document(state: &str) -> Option<String> {
    const MARKER: &str = "active canvas editor: `";
    let start = state.find(MARKER)? + MARKER.len();
    state[start..].find('`').map(|length| state[start..start + length].to_string())
}

/// 맨 위 노드 id를 읽는 `execute` 결과(`TOP <id>`)에서 id를 꺼낸다.
pub fn parse_top_node_id(listed: &str) -> Option<String> {
    let rest = &listed[listed.find("TOP ")? + "TOP ".len()..];
    let id: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    if id.is_empty() { None } else { Some(id) }
}

/// 맨 위 노드 id로 만든 문서 확인 조각. 쓰는 `execute` 입력 맨 앞에 붙이면, 다른 문서로 떨어진 쓰기가 `WRONG_DOCUMENT` 예외로
/// 그 호출 단위에서 되돌려진다.
pub fn guard_snippet(top_node_id: &str) -> String {
    format!(
        "const penDoc=[];Get((n,c)=>{{if(c.depth===0&&n.id===\"{top_node_id}\")penDoc.push(1);c.skipChildren()}});if(!penDoc.length)throw new Error(\"WRONG_DOCUMENT\");"
    )
}

/// 설치된 Pen 앱.
pub struct Pen {
    exe: PathBuf,
    pencil: Pencil,
}

impl Pen {
    /// 설치 위치(`Program Files`, 사용자 `Programs`)에서 `Pen.exe`를 찾는다. 없으면 상태 줄과 같은 안내 문장을 오류로 돌려준다.
    pub fn find() -> Result<Self, String> {
        let program_files = std::env::var("ProgramFiles").unwrap_or_default();
        let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
        pen_install_candidates(&program_files, &local_app_data)
            .into_iter()
            .map(PathBuf::from)
            .find(|exe| exe.is_file())
            .map(|exe| {
                let directory = exe.parent().map(Path::to_path_buf).unwrap_or_default();
                let pencil = Pencil::new(PathBuf::from(pencil_mcp_exe(&directory.to_string_lossy())));
                Self { exe, pencil }
            })
            .ok_or_else(|| pen_open_failure(&PenOpenFailure::NotInstalled))
    }

    /// Pencil 도구 구성. 읽어 둔 구성을 쓰되, Pencil MCP 프로그램 파일의 위치·수정 시각·크기가 그때와 다르면(Pen 업그레이드) 다시 읽는다.
    /// 목록을 못 읽으면 기본 구성(저장 도구 없음, `filePath`)을 돌려준다.
    ///
    /// 왜: 이 프로그램이 떠 있는 동안 Pen이 업그레이드되면 `execute`의 문서 인자(`filePath`→`uri`)와 저장 도구 유무가 바뀐다. 옛 구성을 계속
    /// 쓰면 맞지 않는 인자를 보내 모든 Pencil 호출이 실패한다.
    ///
    /// 읽는 동안 잠금을 쥐어, 동시에 온 요청이 목록을 겹쳐 읽지 않는다.
    pub fn tools(&self) -> PencilTools {
        let stamp = self.pencil.stamp();
        let mut cached = TOOLS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let is_current = cached.as_ref().is_some_and(|known| known.exe == self.pencil.exe && known.stamp == stamp);
        if !is_current {
            *cached = self.pencil.list_tools().map(|tools| CachedTools { exe: self.pencil.exe.clone(), stamp, tools });
        }
        cached.as_ref().map(|known| known.tools.clone()).unwrap_or_default()
    }

    /// 활성 문서 표기. 열린 문서가 없거나 답을 못 받으면 None이다.
    pub fn active_document(&self) -> Option<String> {
        self.pencil.call(PencilTool::GET_APP_STATE, &json!({})).and_then(|state| parse_active_document(&state))
    }

    /// 활성 문서가 된 파일의 문서 확인 조각. 파일 맨 위 노드를 못 읽으면(빈 문서이거나 앱이 아직 못 읽음) None이다.
    pub fn document_guard(
        &self,
        windows_path: &str,
    ) -> Option<String> {
        let document = self.tools().document_argument;
        let mut arguments = json!({
            "description": "Read a top-level node id",
            "input": "let first;Get((n,c)=>{if(c.depth===0&&!first){first=n.id;Print(\"TOP\",first)}c.skipChildren()})"
        });
        arguments[document.key()] = Value::String(document.value(windows_path));
        let listed = self.pencil.call(PencilTool::EXECUTE, &arguments)?;
        parse_top_node_id(&listed).map(|id| guard_snippet(&id))
    }

    /// Pencil 저장 도구(`save{uri}`)로 파일을 저장하고, 도구가 답한 성패를 돌려준다. 답을 `PENCIL_SAVE_DEADLINE`까지 기다린다.
    ///
    /// 성패는 도구의 말일 뿐이다: 답이 없거나 오류여도 파일은 쓰였을 수 있어, 저장 여부는 호출자가 수정 시각으로 판정한다.
    /// 이 경로는 저장 도구가 목록에 있는 Pen 판에서만 쓰이고, 아직 실제 앱에서 불러 본 적이 없다(도구가 목록에 있는지만 확인했다).
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn save_by_tool(
        &self,
        windows_path: &str,
    ) -> ToolOutcome {
        self.pencil.call_for_outcome(
            PencilTool::SAVE,
            &json!({ "uri": to_file_uri(windows_path) }),
            Limits::PENCIL_SAVE_DEADLINE,
        )
    }

    /// 파일을 Pen에 넘긴다. 앱이 실행 중이면 그 창에 열고, 꺼져 있으면 새로 뜨므로 끝나기를 기다리지 않는다.
    ///
    /// 표준 입출력은 모두 닫아 띄운다. Claude가 준 입출력 통로를 앱이 물려받으면, 통로가 끊길 때 앱이 `write EPIPE` 오류 창을 띄운다.
    pub fn request_open(
        &self,
        windows_path: &str,
    ) -> Result<(), String> {
        let mut command = Command::new(&self.exe);
        command.arg(windows_path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }
        command.spawn().map(|_| ()).map_err(|error| pen_open_failure(&PenOpenFailure::LaunchFailed(error.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pencil_1_the_active_document_is_read_from_the_app_state_text() {
        assert_eq!(
            parse_active_document("Editors:\n- active canvas editor: `/Ubuntu/home/u/a.pen`\n").as_deref(),
            Some("/Ubuntu/home/u/a.pen")
        );
        assert_eq!(parse_active_document("active canvas editor: `/C:/w/a b.pen`").as_deref(), Some("/C:/w/a b.pen"));
        assert_eq!(parse_active_document("no editor is open"), None);
        assert_eq!(parse_active_document("active canvas editor: `unterminated"), None);
    }

    #[test]
    fn test_pencil_2_the_top_node_id_becomes_a_guard_that_throws_on_another_document() {
        assert_eq!(parse_top_node_id("TOP aB_9x").as_deref(), Some("aB_9x"));
        assert_eq!(parse_top_node_id("log\nTOP Zk3 \n").as_deref(), Some("Zk3"));
        assert_eq!(parse_top_node_id("nothing here"), None);
        assert_eq!(parse_top_node_id("TOP "), None);
        let guard = guard_snippet("Zk3");
        assert!(guard.contains("n.id===\"Zk3\""));
        assert!(guard.starts_with("const penDoc=[];Get("));
        assert!(guard.ends_with("throw new Error(\"WRONG_DOCUMENT\");"));
    }

    #[test]
    fn test_pencil_3_a_failed_or_error_reply_gives_no_text() {
        let ok = json!({ "id": 2, "result": { "content": [{ "type": "text", "text": "hi" }] } });
        assert_eq!(result_text(&ok).as_deref(), Some("hi"));
        assert_eq!(result_text(&json!({ "id": 2, "error": { "code": 1 } })), None);
        assert_eq!(result_text(&json!({ "id": 2, "result": { "isError": true, "content": [{ "text": "x" }] } })), None);
        assert_eq!(result_text(&json!({ "id": 2, "result": { "content": [] } })), None);
    }

    #[cfg(unix)]
    #[test]
    fn test_pencil_4_a_pencil_program_is_called_in_the_mcp_order_and_ended() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("fake-pencil.sh");
        // 첫 줄(initialize)에 답하고, 알림 한 줄을 지나, 도구 호출에 답한다. 받은 도구 호출 줄은 파일에 남긴다.
        let record = directory.path().join("received.txt");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nread first\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nread note\nread call\nprintf '%s\\n' \"$call\" > '{}'\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"content\":[{{\"type\":\"text\",\"text\":\"active canvas editor: `/C:/w/a.pen`\"}}]}}}}'\nsleep 30\n",
                record.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let pen = Pen { exe: PathBuf::from("unused"), pencil: Pencil::new(script) };
        assert_eq!(pen.active_document().as_deref(), Some("/C:/w/a.pen"));
        let received: Value = serde_json::from_str(std::fs::read_to_string(record).unwrap().trim()).unwrap();
        assert_eq!(received["method"], "tools/call");
        assert_eq!(received["params"]["name"], "get_app_state");
    }

    #[test]
    fn test_pencil_5_a_missing_pencil_program_gives_none() {
        let pencil = Pencil::new(PathBuf::from("/nonexistent/paddock-pencil-mcp"));
        assert_eq!(pencil.call("get_app_state", &json!({})), None);
        assert_eq!(pencil.list_tools(), None);
    }

    #[test]
    fn test_pencil_6_the_tool_list_tells_whether_save_exists_and_which_document_argument_execute_needs() {
        // Pen 1.2.14: 저장 도구가 없고 execute는 filePath를 요구한다.
        let old = PencilTools::from_list(&json!({ "tools": [
            { "name": "browser", "inputSchema": { "type": "object" } },
            { "name": "execute", "inputSchema": { "type": "object", "required": ["filePath", "input"] } },
            { "name": "get_app_state", "inputSchema": { "type": "object" } }
        ] }));
        assert!(!old.has("save"));
        assert!(old.has("execute"));
        assert_eq!(old.document_argument, DocumentArgument::FilePath);
        // 저장 도구가 생긴 판: execute가 filePath 대신 uri를 요구한다.
        let new = PencilTools::from_list(&json!({ "tools": [
            { "name": "execute", "inputSchema": { "required": ["uri", "input"] } },
            { "name": "save", "inputSchema": { "required": ["uri"] } }
        ] }));
        assert!(new.has("save"));
        assert_eq!(new.document_argument, DocumentArgument::Uri);
        // 둘 다 요구하거나 필수 인자를 못 읽으면 지금까지처럼 filePath를 보낸다.
        let both = PencilTools::from_list(&json!({ "tools": [
            { "name": "execute", "inputSchema": { "required": ["uri", "filePath"] } }
        ] }));
        assert_eq!(both.document_argument, DocumentArgument::FilePath);
        assert_eq!(PencilTools::from_list(&json!({})), PencilTools::default());
        assert!(!PencilTools::default().has("save"));
    }

    #[test]
    fn test_pencil_7_windows_paths_become_file_uris() {
        assert_eq!(to_file_uri("C:\\w\\a.pen"), "file:///C:/w/a.pen");
        assert_eq!(to_file_uri("C:\\w\\a b#1.pen"), "file:///C:/w/a%20b%231.pen");
        assert_eq!(to_file_uri("C:\\w\\50%.pen"), "file:///C:/w/50%25.pen");
        assert_eq!(
            to_file_uri("\\\\wsl.localhost\\Ubuntu\\home\\u\\a.pen"),
            "file://wsl.localhost/Ubuntu/home/u/a.pen"
        );
        assert_eq!(to_file_uri("C:\\w\\한.pen"), "file:///C:/w/%ED%95%9C.pen");
        assert_eq!(to_file_uri("/w/a.pen"), "file:///w/a.pen");
        assert_eq!(DocumentArgument::Uri.value("C:\\w\\a.pen"), "file:///C:/w/a.pen");
        assert_eq!(DocumentArgument::FilePath.value("C:\\w\\a.pen"), "C:\\w\\a.pen");
        assert_eq!((DocumentArgument::FilePath.key(), DocumentArgument::Uri.key()), ("filePath", "uri"));
    }

    #[cfg(unix)]
    #[test]
    fn test_pencil_8_the_tool_list_is_read_with_a_tools_list_request() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("fake-pencil.sh");
        let record = directory.path().join("received.txt");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nread first\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nread note\nread call\nprintf '%s\\n' \"$call\" > '{}'\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"tools\":[{{\"name\":\"save\",\"inputSchema\":{{\"required\":[\"uri\"]}}}}]}}}}'\nsleep 30\n",
                record.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let tools = Pencil::new(script).list_tools().expect("a tool list");
        assert!(tools.has("save"));
        let received: Value = serde_json::from_str(std::fs::read_to_string(record).unwrap().trim()).unwrap();
        assert_eq!(received["method"], "tools/list");
    }

    #[test]
    fn test_pencil_9_only_an_error_reply_is_a_failed_tool_call() {
        let reply = |value: Value| tool_outcome(Some(&value));
        assert_eq!(
            reply(json!({ "result": { "content": [{ "type": "text", "text": "ok" }] } })),
            ToolOutcome::Succeeded
        );
        assert_eq!(reply(json!({ "result": { "content": [] } })), ToolOutcome::Succeeded);
        assert_eq!(reply(json!({ "result": {} })), ToolOutcome::Succeeded);
        assert_eq!(reply(json!({ "error": { "message": "bad uri" } })), ToolOutcome::Failed("bad uri".into()));
        assert_eq!(
            reply(json!({ "result": { "isError": true, "content": [{ "text": "no file" }] } })),
            ToolOutcome::Failed("no file".into())
        );
        assert_eq!(reply(json!({ "error": {} })), ToolOutcome::Failed("unknown error".into()));
        assert_eq!(tool_outcome(None), ToolOutcome::NoAnswer);
    }

    #[cfg(unix)]
    #[test]
    fn test_pencil_10_the_tool_list_is_read_again_when_the_pencil_program_changes() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("fake-pencil.sh");
        // 도구 목록 요청에 `tools` 배열을 답하는 가짜 Pencil 프로그램. 판이 바뀌면 파일 내용(크기)이 바뀐다.
        let write_version = |tools: &str| {
            std::fs::write(
                &script,
                format!(
                    "#!/bin/sh\nread first\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nread note\nread call\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"tools\":{tools}}}}}'\nsleep 30\n"
                ),
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        write_version(r#"[{"name":"execute","inputSchema":{"required":["filePath","input"]}}]"#);
        let pen = Pen { exe: PathBuf::from("unused"), pencil: Pencil::new(script.clone()) };
        let old = pen.tools();
        assert!(!old.has("save"));
        assert_eq!(old.document_argument, DocumentArgument::FilePath);
        write_version(
            r#"[{"name":"execute","inputSchema":{"required":["uri","input"]}},{"name":"save","inputSchema":{"required":["uri"]}}]"#,
        );
        let new = pen.tools();
        assert!(new.has("save"));
        assert_eq!(new.document_argument, DocumentArgument::Uri);
    }
}
