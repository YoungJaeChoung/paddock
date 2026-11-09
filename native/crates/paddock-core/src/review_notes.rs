//! 검토 메모: 코드를 보다 줄에 남긴 메모를 모아 에이전트에게 한 번에 붙여 넣을 글로 바꾸는 순수 규칙.
//!
//! 에이전트에게 "이 파일 이 줄을 이렇게 고쳐"를 말로 풀어 쓰는 대신 메모마다 파일·줄·그 줄의 내용을 붙여 한 덩어리로 넘기려고 존재한다.
//! 줄 내용을 함께 보내는 이유는 에이전트가 파일을 고친 뒤에도 메모가 가리키는 줄을 알아볼 수 있게 하기 위해서다.
//! 메모는 앱 창 안에서만 모으고 파일로 남기지 않으며 보내면 비운다.
use crate::paths::relative_to;

/// 메모 개수·글자 수 한도.
pub mod limits {
    /// 한 줄 내용을 글에 담을 때 자르는 길이. 긴 줄(번들·데이터)이 글 전체를 키우지 않게 한다.
    pub const MAX_LINE_CHARS: usize = 160;
    /// 한 번에 보낼 수 있는 메모 수. 넘으면 보내기 전에 나눠 보내도록 알린다.
    pub const MAX_NOTES: usize = 50;
}

/// 메모 하나. 줄 번호는 1부터다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// 작업 폴더 기준 상대 경로(작업 폴더 밖이면 원래 경로).
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
    /// 시작 줄의 내용.
    pub line_text: String,
    pub text: String,
}

/// 파일 경로를 작업 폴더 기준 상대 경로로 바꾼다. 어느 작업 폴더에도 속하지 않으면 원래 경로 그대로다. 폴더 경계를 지켜 `/repo`가 `/repo-other/a.js`에 맞지 않는다.
///
/// Examples
/// --------
/// | file               | roots      | 결과               |
/// |--------------------|------------|--------------------|
/// | `/repo/src/a.js`   | `/repo`    | `src/a.js`         |
/// | `/repo/src/a.js`   | `/repo/`   | `src/a.js`         |
/// | `/repo-other/a.js` | `/repo`    | `/repo-other/a.js` |
/// | `/x/a.js`          | (없음)     | `/x/a.js`          |
pub fn relative_path(
    file: &str,
    roots: &[String],
) -> String {
    roots
        .iter()
        .find_map(|root| relative_to(root, file).filter(|relative| !relative.is_empty()))
        .unwrap_or_else(|| file.to_string())
}

/// 줄 내용의 앞뒤 공백을 걷고 `limits::MAX_LINE_CHARS`글자에서 자른다(잘리면 `…`).
fn shorten(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() > limits::MAX_LINE_CHARS {
        let head: String = trimmed.chars().take(limits::MAX_LINE_CHARS).collect();
        format!("{head}\u{2026}")
    } else {
        trimmed.to_string()
    }
}

/// 메모 목록을 에이전트 터미널에 붙여 넣을 글로 만든다. 메모가 없으면 빈 글자다. 여러 줄을 고른 메모는 `시작-끝`으로 쓰고 첫 줄 내용만 붙인다.
/// 글은 줄바꿈으로 끝나지만 Enter를 누르는 것은 아니다(붙여넣기 감싸기를 켠 프로그램은 그대로 입력칸에 쌓는다).
///
/// Examples
/// --------
/// | 메모                                         | 결과                                                              |
/// |----------------------------------------------|-------------------------------------------------------------------|
/// | `src/a.js:3`, 줄 `  return x;`, `null check`  | `Review notes (1):\n\n1. src/a.js:3\n   > return x;\n   null check\n` |
/// | `b.js:5-9`, 줄 `if (a) {`, `split`            | `… 1. b.js:5-9\n   > if (a) {\n   split\n`                        |
/// | (없음)                                       | (빈 글자)                                                         |
pub fn format_notes(notes: &[Note]) -> String {
    if notes.is_empty() {
        String::new()
    } else {
        let blocks: Vec<String> = notes
            .iter()
            .enumerate()
            .map(|(index, note)| {
                let range = if note.end_line > note.start_line {
                    format!("{}-{}", note.start_line, note.end_line)
                } else {
                    note.start_line.to_string()
                };
                let indented = note.text.trim().split('\n').collect::<Vec<_>>().join("\n   ");
                format!("{}. {}:{range}\n   > {}\n   {indented}", index + 1, note.file, shorten(&note.line_text))
            })
            .collect();
        format!("Review notes ({}):\n\n{}\n", notes.len(), blocks.join("\n\n"))
    }
}

/// 고른 글자의 시작·끝 위치(줄·열 모두 0부터)에서 메모가 가리킬 줄 범위(1부터)를 구한다.
/// 끝이 줄 맨 앞이면(줄 전체를 끌어 골랐다) 그 줄은 범위에 넣지 않는다.
///
/// Examples
/// --------
/// | 시작(줄,열) | 끝(줄,열) | 결과    |
/// |-------------|-----------|---------|
/// | (2, 4)      | (2, 9)    | (3, 3)  |
/// | (2, 0)      | (4, 0)    | (3, 4)  |
/// | (2, 0)      | (4, 7)    | (3, 5)  |
/// | (2, 5)      | (2, 5)    | (3, 3)  |
pub fn line_range_of(
    start: (usize, usize),
    end: (usize, usize),
) -> (usize, usize) {
    let start_line = start.0 + 1;
    let end_line = if end.1 == 0 && end.0 > start.0 { end.0 } else { end.0 + 1 };
    (start_line, end_line.max(start_line))
}

/// 메모 입력 창의 제목.
///
/// Examples
/// --------
/// | start | end | 결과                |
/// |-------|-----|---------------------|
/// | 3     | 3   | `Note for line 3`   |
/// | 3     | 5   | `Note for lines 3-5`|
pub fn prompt_title(
    start_line: usize,
    end_line: usize,
) -> String {
    if start_line == end_line {
        format!("Note for line {start_line}")
    } else {
        format!("Note for lines {start_line}-{end_line}")
    }
}

/// 메모를 보낼 터미널의 결정.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendTarget {
    /// 이 터미널로 보낸다.
    One(u32),
    /// 에이전트가 여럿이고 지금 보는 터미널이 에이전트가 아니라서 고르게 한다.
    Pick(Vec<u32>),
    /// 에이전트 터미널이 없다. 메모는 그대로 둔다.
    None,
}

/// 지금 보는 터미널이 에이전트면 그곳으로, 아니면 에이전트가 하나일 때 그곳으로, 여럿이면 고르게 하고, 없으면 `None`.
///
/// Examples
/// --------
/// | current | agents   | 결과           |
/// |---------|----------|----------------|
/// | 2       | [1, 2]   | `One(2)`       |
/// | 3       | [1, 2]   | `Pick([1, 2])` |
/// | 3       | [1]      | `One(1)`       |
/// | 3       | []       | `None`         |
pub fn send_target(
    current: Option<u32>,
    agents: &[u32],
) -> SendTarget {
    match (current.filter(|id| agents.contains(id)), agents) {
        (Some(id), _) => SendTarget::One(id),
        (None, []) => SendTarget::None,
        (None, [only]) => SendTarget::One(*only),
        (None, many) => SendTarget::Pick(many.to_vec()),
    }
}

/// 열 수 없는 때(편집기·비교가 없다) 알리는 문장.
pub const NO_EDITOR: &str = "Open a file or a diff and select the lines first.";
/// 보낼 에이전트 터미널이 없을 때 알리는 문장.
pub const NO_AGENT: &str =
    "No agent is running in a terminal. Start Claude or Codex in a terminal first; your notes are kept.";
/// 붙여 넣은 뒤 알리는 문장.
pub const PASTED: &str = "Review notes pasted into the agent terminal. Press Enter there to send.";
/// 메모를 모두 비운 뒤 알리는 문장.
pub const CLEARED: &str = "Review notes cleared.";
/// 메모 입력칸의 안내 글.
pub const INPUT_PLACEHOLDER: &str = "What should the agent change here?";
/// 에이전트를 고르는 메뉴의 안내 글.
pub const PICK_PLACEHOLDER: &str = "Send review notes to which agent?";

/// 메모가 가득 찼을 때 알리는 문장.
pub fn full_text() -> String {
    format!("Send the {} notes first, then add more.", limits::MAX_NOTES)
}

/// 메모를 더한 뒤 알리는 문장.
pub fn added_text(n_notes: usize) -> String {
    format!("Review note added ({n_notes}).")
}

/// 상태 줄의 메모 개수 표시.
pub fn status_text(n_notes: usize) -> String {
    format!("Review notes: {n_notes}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(
        file: &str,
        start_line: usize,
        end_line: usize,
        line_text: &str,
        text: &str,
    ) -> Note {
        Note { file: file.into(), start_line, end_line, line_text: line_text.into(), text: text.into() }
    }

    #[test]
    fn test_rn01_a_path_inside_a_work_folder_becomes_relative_and_the_folder_boundary_is_respected() {
        let roots = |list: &[&str]| list.iter().map(|root| root.to_string()).collect::<Vec<_>>();
        assert_eq!(relative_path("/repo/src/a.js", &roots(&["/repo"])), "src/a.js");
        assert_eq!(relative_path("/repo/src/a.js", &roots(&["/repo/"])), "src/a.js");
        assert_eq!(relative_path("/repo-other/a.js", &roots(&["/repo"])), "/repo-other/a.js");
        assert_eq!(relative_path("/x/a.js", &[]), "/x/a.js");
    }

    #[test]
    fn test_rn02_notes_are_numbered_with_file_line_range_and_the_first_line_text() {
        let text = format_notes(&[
            note("src/a.js", 3, 3, "  return x;", "null check"),
            note("b.js", 5, 9, "if (a) {", "split\nthis up"),
        ]);
        assert_eq!(
            text,
            "Review notes (2):\n\n1. src/a.js:3\n   > return x;\n   null check\n\n2. b.js:5-9\n   > if (a) {\n   split\n   this up\n"
        );
    }

    #[test]
    fn test_rn03_no_notes_make_no_text_and_long_lines_are_cut() {
        assert_eq!(format_notes(&[]), "");
        let text = format_notes(&[note("a", 1, 1, &"x".repeat(500), "t")]);
        assert!(text.contains(&format!("{}\u{2026}", "x".repeat(160))));
        assert!(!text.contains(&"x".repeat(161)));
        // 160글자는 글자 수로 센다(한글 한 글자도 한 글자).
        let korean = format_notes(&[note("a", 1, 1, &"가".repeat(161), "t")]);
        assert!(korean.contains(&format!("{}\u{2026}", "가".repeat(160))));
    }

    #[test]
    fn test_c_notes_1_a_selection_ending_at_a_line_start_leaves_that_line_out() {
        assert_eq!(line_range_of((2, 4), (2, 9)), (3, 3));
        assert_eq!(line_range_of((2, 0), (4, 0)), (3, 4));
        assert_eq!(line_range_of((2, 0), (4, 7)), (3, 5));
        assert_eq!(line_range_of((2, 5), (2, 5)), (3, 3));
        assert_eq!(line_range_of((2, 0), (2, 0)), (3, 3));
    }

    #[test]
    fn test_c_notes_2_the_prompt_names_one_line_or_a_range() {
        assert_eq!(prompt_title(3, 3), "Note for line 3");
        assert_eq!(prompt_title(3, 5), "Note for lines 3-5");
    }

    #[test]
    fn test_c_notes_3_the_target_prefers_the_current_agent_then_a_single_agent_then_asks() {
        assert_eq!(send_target(Some(2), &[1, 2]), SendTarget::One(2));
        assert_eq!(send_target(Some(3), &[1, 2]), SendTarget::Pick(vec![1, 2]));
        assert_eq!(send_target(None, &[1, 2]), SendTarget::Pick(vec![1, 2]));
        assert_eq!(send_target(Some(3), &[1]), SendTarget::One(1));
        assert_eq!(send_target(Some(3), &[]), SendTarget::None);
    }

    #[test]
    fn test_c_notes_4_sentences_match_the_old_apps() {
        assert_eq!(full_text(), "Send the 50 notes first, then add more.");
        assert_eq!(added_text(2), "Review note added (2).");
        assert_eq!(status_text(3), "Review notes: 3");
    }
}
