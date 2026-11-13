//! 텍스트 보기: 편집기(줄 번호 거터 + 구문 강조). 고칠 수 있는 파일이면 글자를 고칠 수 있고, 저장은 앱이 `encoded`로 BOM·줄바꿈을 되살려 쓴다.
//!
//! 편집기 부품이 색을 앱 테마에서 읽으므로, 강조 색과 바탕·글자·줄 번호 색은 `highlight_theme`가 만든 테마를 부품 테마에 얹어 쓴다.
use std::sync::Arc;

use gpui_kit::component::highlighter::HighlightTheme;
use gpui_kit::component::input::{Editor, EditorState, InputEvent, Position};
use gpui_kit::{AppContext, Context, Entity, IntoElement, ParentElement, Styled, Subscription, Window, div};
use paddock_core::file_kind::{extension_of, highlight_language_of};
use paddock_core::text_file::Encoded;
use paddock_core::theme::{Colors, Mode, file_view};
use serde_json::json;

use crate::chrome::zpx;
use crate::window_view::WorkWindow;

/// 구문 강조 언어가 없는 파일의 언어 이름(강조 없는 일반 글자).
const PLAIN_LANGUAGE: &str = "text";

/// 이 파일의 글자를 고칠 수 있는가: 텍스트·Markdown 원문이고, UTF-8이 아닌 바이트를 바꿔 읽지 않았으며(저장하면 원래 바이트가 사라진다),
/// 사용자가 `Open as text anyway`로 억지로 연 파일(이진일 수 있다)이 아니어야 한다.
///
/// Examples
/// --------
/// | kind     | lossy | 억지로 연 파일 | 결과  |
/// |----------|-------|----------------|-------|
/// | Text     | false | false          | true  |
/// | Markdown | false | false          | true  |
/// | Text     | true  | false          | false |
/// | Text     | false | true           | false |
/// | Image    | false | false          | false |
pub fn is_editable(
    kind: paddock_core::file_kind::FileKind,
    encoded: &Encoded,
    forced_text: bool,
) -> bool {
    paddock_core::text_file::can_edit(kind) && !encoded.lossy && !forced_text
}

/// 읽어 낸 텍스트 파일 하나: 글자, 파일 크기, 저장할 때 되살릴 BOM·줄바꿈 종류, 고칠 수 있는지(`is_editable`).
pub struct TextFile {
    pub text: String,
    pub size: u64,
    pub encoded: Encoded,
    pub editable: bool,
}

/// 열어 둔 텍스트 하나: 편집기 상태와 상태 글에 보일 요약.
pub struct TextBody {
    pub editor: Entity<EditorState>,
    pub n_lines: usize,
    /// 강조에 쓰는 언어 이름. 문법이 없는 파일은 `text`다.
    pub language: &'static str,
    pub size: u64,
    /// 읽을 때 기억한 BOM·줄바꿈 종류. 저장할 때 되살린다.
    pub encoded: Encoded,
    /// 글자를 고칠 수 있는가. 텍스트·Markdown 원문이 아니거나 UTF-8이 아닌 바이트를 바꿔 읽은 파일이면 읽기 전용이다.
    pub editable: bool,
    /// 편집기의 글자 변경을 창에 알리는 구독(없애면 알림이 끊긴다).
    _changes: Subscription,
}

impl TextBody {
    /// 디스크의 파일이 바뀌어 다시 읽은 글자로 바꾼다. 스크롤 위치는 그대로 둔다(내용이 짧아지면 부품이 끝으로 맞춘다).
    pub fn replace(
        &mut self,
        file: TextFile,
        window: &mut Window,
        cx: &mut Context<WorkWindow>,
    ) {
        let TextFile { text, size, encoded, editable } = file;
        self.n_lines = text.lines().count().max(1);
        self.size = size;
        self.encoded = encoded;
        self.editable = editable;
        self.editor.update(cx, |state, cx| {
            let offset = state.scroll_offset();
            state.set_value(text, window, cx);
            state.set_scroll_offset(offset, cx);
        });
    }
}

/// 앱 색 묶음에서 편집기 테마를 만든다: 바탕·글자·줄 번호 색과 구문 강조 여섯 색(키워드·문자열·주석·숫자·타입·함수).
///
/// 부품의 강조 이름 중 같은 계열은 같은 색을 쓴다(예: 불리언·태그는 키워드 색, 상수는 숫자 색).
pub fn highlight_theme(
    colors: &Colors,
    mode: Mode,
) -> Option<Arc<HighlightTheme>> {
    let hex = |color: u32| format!("#{color:06x}");
    let style = |color: u32| json!({"color": hex(color)});
    let theme = json!({
        "name": "Paddock",
        "appearance": match mode {
            Mode::Dark => "dark",
            Mode::Light => "light",
        },
        "style": {
            "editor.background": hex(colors.terminal_bg),
            "editor.foreground": hex(colors.terminal_fg),
            "editor.line_number": hex(colors.gutter_fg),
            "editor.active_line_number": hex(colors.fg_strong),
            "syntax": {
                "keyword": style(colors.syntax_keyword),
                "boolean": style(colors.syntax_keyword),
                "preproc": style(colors.syntax_keyword),
                "label": style(colors.syntax_keyword),
                "tag": style(colors.syntax_keyword),
                "title": style(colors.syntax_keyword),
                "string": style(colors.syntax_string),
                "string.regex": style(colors.syntax_string),
                "string.special": style(colors.syntax_string),
                "string.escape": style(colors.syntax_string),
                "text.literal": style(colors.syntax_string),
                "link_uri": style(colors.syntax_string),
                "comment": style(colors.syntax_comment),
                "comment.doc": style(colors.syntax_comment),
                "number": style(colors.syntax_number),
                "constant": style(colors.syntax_number),
                "type": style(colors.syntax_type),
                "constructor": style(colors.syntax_type),
                "enum": style(colors.syntax_type),
                "variant": style(colors.syntax_type),
                "attribute": style(colors.syntax_type),
                "function": style(colors.syntax_function),
            },
        },
    });
    serde_json::from_value::<HighlightTheme>(theme).ok().map(Arc::new)
}

/// 읽은 텍스트로 편집기를 만든다. 확장자에 맞는 문법이 있고 파일이 `HIGHLIGHT_LIMIT` 이하이면 강조하고, 줄 번호와 코드 접기 단추(문법이 있는 언어만)를 보이며, 긴 줄은 줄바꿈 없이 가로로 스크롤한다.
/// `editable`이 참이면 글자를 고칠 수 있고, 고치면 창에 그 파일이 저장되지 않은 상태가 됐다고 알린다. 찾기·바꾸기 패널은 항상 쓸 수 있다.
pub fn build_text(
    file: TextFile,
    path: &str,
    window: &mut Window,
    cx: &mut Context<WorkWindow>,
) -> TextBody {
    let TextFile { text, size, encoded, editable } = file;
    let language = highlight_language_of(&extension_of(path), size).unwrap_or(PLAIN_LANGUAGE);
    let n_lines = text.lines().count().max(1);
    let editor = cx.new(|cx| {
        EditorState::new(window, cx)
            .language(language)
            .line_number(true)
            .soft_wrap(false)
            .folding(true)
            .auto_close(false)
            .smart_indent(false)
            .searchable(true)
            .default_value(text)
    });
    // 변경 알림은 편집기 자신으로 파일을 찾는다: 이름이 바뀐 파일의 편집기는 새 경로 밑으로 옮겨 가므로 만들 때의 경로를 쥐고 있으면 안 된다.
    let _changes = cx.subscribe_in(&editor, window, move |this: &mut WorkWindow, editor, event: &InputEvent, _, cx| {
        if matches!(event, InputEvent::Change) {
            this.note_edited(editor.entity_id(), cx);
        }
    });
    super::review_notes::route_context_menu(&editor, cx);
    TextBody { editor, n_lines, language, size, encoded, editable, _changes }
}

/// 텍스트 보기 그리기. 읽기 전용 보기는 키를 쳐도 내용이 바뀌지 않는다.
pub fn text_view(body: &TextBody) -> impl IntoElement {
    text_view_of(&body.editor, body.editable)
}

/// 편집기 `editor`를 텍스트 보기로 그린다(파일 분리 창은 `TextBody` 없이 편집기 핸들만 쥔다).
pub fn text_view_of(
    editor: &Entity<EditorState>,
    editable: bool,
) -> impl IntoElement {
    div()
        .size_full()
        .pt(zpx(file_view::PADDING_Y))
        .pb(zpx(file_view::PADDING_Y))
        .child(Editor::new(editor).readonly(!editable).appearance(false).bordered(false).h(gpui_kit::relative(1.0)))
}

/// 목표 줄 위에 남기는 줄 수. 일치 줄이 화면 맨 위에 붙지 않고 앞뒤 맥락과 함께 보이게 한다.
const CONTEXT_LINES_ABOVE: usize = 3;

/// `line`번째(1부터) 줄 앞에 커서를 두고 그 줄이 화면 위쪽에 보이게 스크롤한다. 편집기가 아직 한 번도 그려지지 않았으면
/// (줄 높이를 모르면) 아무것도 하지 않고 false를 돌려준다 — 부르는 쪽이 그려진 뒤에 다시 부른다.
pub fn scroll_to_line(
    body: &TextBody,
    line: usize,
    window: &mut Window,
    cx: &mut Context<WorkWindow>,
) -> bool {
    let line_height = body.editor.read(cx).line_height();
    if let Some(line_height) = line_height {
        let position = Position::new(u32::try_from(line.saturating_sub(1)).unwrap_or(0), 0);
        let n_rows_above = line.saturating_sub(1 + CONTEXT_LINES_ABOVE);
        body.editor.update(cx, |state, cx| {
            state.set_cursor_position(position, window, cx);
            state.set_scroll_offset(gpui_kit::point(Default::default(), -(line_height * n_rows_above as f32)), cx);
        });
    }
    line_height.is_some()
}

#[cfg(test)]
mod tests {
    use paddock_core::theme::Mode;

    use super::*;

    #[test]
    fn test_the_highlight_theme_builds_from_both_color_sets_and_carries_the_six_syntax_colors() {
        for mode in Mode::ALL {
            let theme = highlight_theme(mode.colors(), mode).expect("a valid theme");
            assert_eq!(theme.name, "Paddock");
            let color = |name: &str| theme.style.syntax.style(name).and_then(|style| style.color);
            assert!(color("keyword").is_some());
            assert_eq!(color("string"), color("string.regex"));
            assert_ne!(color("keyword"), color("comment"));
        }
    }
}
