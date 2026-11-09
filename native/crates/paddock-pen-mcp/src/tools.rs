//! 세 도구(`pen_open`·`pen_save`·`pen_close`)의 이름과 설명·입력 스키마. 실행은 하지 않는 순수 자료다.
use serde_json::{Value, json};

/// 도구 이름.
pub struct ToolName;

impl ToolName {
    pub const OPEN: &'static str = "pen_open";
    pub const SAVE: &'static str = "pen_save";
    pub const CLOSE: &'static str = "pen_close";
}

const PATH_DESCRIPTION: &str = "Path of the .pen file. Accepts Windows, UNC and WSL (/home/...) paths.";

/// 알고 있는 도구 이름인지.
pub fn is_known(name: &str) -> bool {
    [ToolName::OPEN, ToolName::SAVE, ToolName::CLOSE].contains(&name)
}

/// `tools/list`가 돌려줄 도구 목록. 앞 두 도구는 `path`만 받고, `pen_close`는 `discard`도 받는다.
pub fn list() -> Value {
    json!([
        {
            "name": ToolName::OPEN,
            "description": "Opens a .pen file in the Pen desktop app and checks that it became the active document. Call it when you first work with a file, or when a Pencil tool fails with \"A file needs to be open in the editor\". Pass the filePath (or uri, when the result gives one) and guard from the result to execute, and you can work on several files at once regardless of the active document. If it fails, do not write with Pencil.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": PATH_DESCRIPTION } },
                "required": ["path"]
            }
        },
        {
            "name": ToolName::SAVE,
            "description": "Saves the changes of a .pen file from the Pen desktop app to disk. Content written with Pencil tools lives only in app memory until it is saved, so call this when you finish a unit of work. It makes the file the active document and chooses Save in the app's File menu without keyboard input (a minimized window is shown briefly and minimized again), or uses Pencil's own save tool when the app offers one, so call it only at the end of a unit of work, not after every write. It confirms the save by the file's modified time, fails if another file this tool opened changed instead, and refuses a file the app read as an empty document so that the file is not overwritten.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": PATH_DESCRIPTION } },
                "required": ["path"]
            }
        },
        {
            "name": ToolName::CLOSE,
            "description": "Closes the Pen window of a .pen file to give its memory back. The app opens one window per file and each window uses a lot of memory, so close files you are done with. If you wrote content, call pen_save first. If it fails because of a save prompt and you wrote nothing, call it again with discard: true. discard throws away unsaved changes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": PATH_DESCRIPTION },
                    "discard": {
                        "type": "boolean",
                        "description": "When a save prompt appears, close without saving. By default (false) the prompt is cancelled and the window stays open."
                    }
                },
                "required": ["path"]
            }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tools_1_the_list_has_the_three_tools_each_requiring_a_path() {
        let tools = list();
        let names: Vec<&str> = tools.as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["pen_open", "pen_save", "pen_close"]);
        for tool in tools.as_array().unwrap() {
            assert_eq!(tool["inputSchema"]["required"], json!(["path"]));
            assert!(is_known(tool["name"].as_str().unwrap()));
        }
        assert_eq!(tools[2]["inputSchema"]["properties"]["discard"]["type"], "boolean");
        assert!(!is_known("pen_delete"));
    }
}
