//! 줄 하나의 JSON-RPC 요청을 응답 줄로 바꾸는 순수 규칙. 도구 실행은 `ToolRunner`에 맡긴다.
//!
//! 응답은 줄 단위 JSON이다. `id`가 없는 알림에는 답하지 않고, 읽지 못한 줄에는 `-32700`, 모르는 메서드·도구에는 `-32601`로 답한다.
//! 도구 실행의 실패는 JSON-RPC 오류가 아니라 `isError` 결과다(에이전트가 문장을 읽고 다음 행동을 정한다).
use serde_json::{Value, json};

use crate::tools;

/// 요청이 프로토콜 판을 말하지 않을 때 돌려주는 판.
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

/// 도구를 실제로 실행하는 쪽. 이름은 `tools::is_known`을 통과한 것만 온다.
pub trait ToolRunner {
    /// 도구 하나를 실행해 결과 글을 돌려준다. 실패하면 에이전트에게 보일 문장을 오류로 돌려준다.
    fn run(
        &self,
        name: &str,
        arguments: &Value,
    ) -> Result<String, String>;
}

/// 메서드 하나를 처리해 결과를 만든다. 오류 문장은 `-32601` 응답이 된다.
fn handle(
    message: &Value,
    runner: &impl ToolRunner,
) -> Result<Value, String> {
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").unwrap_or(&Value::Null);
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL_VERSION),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "pen-open", "version": env!("CARGO_PKG_VERSION") }
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::list() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            if tools::is_known(name) {
                let arguments = params.get("arguments").unwrap_or(&Value::Null);
                Ok(match runner.run(name, arguments) {
                    Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                    Err(text) => json!({ "isError": true, "content": [{ "type": "text", "text": text }] }),
                })
            } else {
                Err(format!("Unknown tool: {name}"))
            }
        }
        other => Err(format!("Unsupported method: {other}")),
    }
}

/// 입력 줄 하나에 대한 응답 줄. 빈 줄(BOM만 있는 줄 포함)과 `id` 없는 알림에는 답이 없어 None이다.
///
/// Examples
/// --------
/// | 입력 줄                                          | 결과                                   |
/// |--------------------------------------------------|----------------------------------------|
/// | (빈 줄)·BOM만 있는 줄                            | None                                   |
/// | `{"jsonrpc":"2.0","method":"notifications/initialized"}` | None                           |
/// | `{"id":1,"method":"ping"}`                       | `{"jsonrpc":"2.0","id":1,"result":{}}` |
/// | BOM이 앞에 붙은 `{"id":1,"method":"ping"}`       | 위와 같다                              |
/// | `not json`                                       | `id: null`, 코드 `-32700`              |
/// | `{"id":2,"method":"tools/call","params":{"name":"x"}}` | 코드 `-32601`                    |
pub fn respond(
    line: &str,
    runner: &impl ToolRunner,
) -> Option<String> {
    let text = line.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        None
    } else {
        match serde_json::from_str::<Value>(text) {
            Err(_) => Some(
                json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "Could not parse the JSON." } })
                    .to_string(),
            ),
            Ok(message) => message.get("id").map(|id| {
                let reply = match handle(&message, runner) {
                    Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": error } }),
                };
                reply.to_string()
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    impl ToolRunner for Echo {
        fn run(
            &self,
            name: &str,
            arguments: &Value,
        ) -> Result<String, String> {
            match arguments.get("path").and_then(Value::as_str) {
                Some("fail") => Err(format!("{name} failed")),
                Some(path) => Ok(format!("{name} {path}")),
                None => Err("path is required.".to_string()),
            }
        }
    }

    fn reply(line: &str) -> Value {
        serde_json::from_str(&respond(line, &Echo).expect("a reply")).unwrap()
    }

    #[test]
    fn test_rpc_1_initialize_echoes_the_protocol_version_and_names_the_server() {
        let result =
            reply(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#);
        assert_eq!(result["id"], 1);
        assert_eq!(result["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(result["result"]["serverInfo"]["name"], "pen-open");
        assert_eq!(result["result"]["capabilities"], json!({ "tools": {} }));
        let bare = reply(r#"{"id":2,"method":"initialize"}"#);
        assert_eq!(bare["result"]["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn test_rpc_2_ping_and_the_tool_list_answer_with_their_ids() {
        assert_eq!(reply(r#"{"id":"a","method":"ping"}"#), json!({ "jsonrpc": "2.0", "id": "a", "result": {} }));
        let list = reply(r#"{"id":3,"method":"tools/list"}"#);
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_rpc_3_notifications_blank_lines_and_a_lone_bom_get_no_answer() {
        assert_eq!(respond(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, &Echo), None);
        assert_eq!(respond("", &Echo), None);
        assert_eq!(respond("   ", &Echo), None);
        assert_eq!(respond("\u{feff}", &Echo), None);
    }

    #[test]
    fn test_rpc_4_a_byte_order_mark_before_the_json_is_ignored() {
        let result = reply("\u{feff}{\"id\":1,\"method\":\"ping\"}");
        assert_eq!(result["result"], json!({}));
    }

    #[test]
    fn test_rpc_5_unreadable_json_answers_32700_with_a_null_id() {
        let result = reply("not json");
        assert_eq!(result["id"], Value::Null);
        assert_eq!(result["error"]["code"], -32700);
    }

    #[test]
    fn test_rpc_6_an_unknown_tool_or_method_answers_32601() {
        let tool =
            reply(r#"{"id":4,"method":"tools/call","params":{"name":"pen_delete","arguments":{"path":"a.pen"}}}"#);
        assert_eq!(tool["error"]["code"], -32601);
        assert_eq!(tool["error"]["message"], "Unknown tool: pen_delete");
        let method = reply(r#"{"id":5,"method":"resources/list"}"#);
        assert_eq!(method["error"]["message"], "Unsupported method: resources/list");
    }

    #[test]
    fn test_rpc_7_a_tool_result_is_text_content_and_a_tool_failure_is_an_error_result() {
        let ok =
            reply(r#"{"id":6,"method":"tools/call","params":{"name":"pen_open","arguments":{"path":"C:\\a.pen"}}}"#);
        assert_eq!(ok["result"]["content"][0]["text"], "pen_open C:\\a.pen");
        assert_eq!(ok["result"].get("isError"), None);
        let failed =
            reply(r#"{"id":7,"method":"tools/call","params":{"name":"pen_save","arguments":{"path":"fail"}}}"#);
        assert_eq!(failed["result"]["isError"], true);
        assert_eq!(failed["result"]["content"][0]["text"], "pen_save failed");
        assert!(failed.get("error").is_none());
        let missing = reply(r#"{"id":8,"method":"tools/call","params":{"name":"pen_close"}}"#);
        assert_eq!(missing["result"]["isError"], true);
    }
}
