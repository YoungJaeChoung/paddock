//! 텍스트 보기: 읽기 전용 편집기(줄 번호 거터 + 구문 강조). 파일을 고치지 않는다.
//!
//! 편집기 부품이 색을 앱 테마에서 읽으므로, 강조 색과 바탕·글자·줄 번호 색은 `highlight_theme`가 만든 테마를 부품 테마에 얹어 쓴다.
use std::sync::Arc;

use gpui_kit::component::highlighter::HighlightTheme;
use gpui_kit::component::input::{Editor, EditorState, Position};
use gpui_kit::{AppContext, Context, Entity, IntoElement, ParentElement, Styled, Window, div};
use paddock_core::file_kind::{extension_of, highlight_language_of};
use paddock_core::theme::{Colors, Mode, file_view};
use serde_json::json;

use crate::chrome::zpx;
use crate::window_view::WorkWindow;

/// 구문 강조 언어가 없는 파일의 언어 이름(강조 없는 일반 글자).
const PLAIN_LANGUAGE: &str = "text";

/// 열어 둔 텍스트 하나: 편집기 상태와 상태 글에 보일 요약.
pub struct TextBody {
    pub editor: Entity<EditorState>,
    pub n_lines: usize,
    /// 강조에 쓰는 언어 이름. 문법이 없는 파일은 `text`다.
    pub language: &'static str,
    pub size: u64,
}

impl TextBody {
    /// 디스크의 파일이 바뀌어 다시 읽은 글자로 바꾼다. 스크롤 위치는 그대로 둔다(내용이 짧아지면 부품이 끝으로 맞춘다).
    pub fn replace(
        &mut self,
        text: String,
        size: u64,
        window: &mut Window,
        cx: &mut Context<WorkWindow>,
    ) {
        self.n_lines = text.lines().count().max(1);
        self.size = size;
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

/// 읽은 텍스트로 편집기를 만든다. 확장자에 맞는 문법이 있고 파일이 `HIGHLIGHT_LIMIT` 이하이면 강조하고, 줄 번호를 보이며, 긴 줄은 줄바꿈 없이 가로로 스크롤한다.
pub fn build_text(
    text: String,
    size: u64,
    path: &str,
    window: &mut Window,
    cx: &mut Context<WorkWindow>,
) -> TextBody {
    let language = highlight_language_of(&extension_of(path), size).unwrap_or(PLAIN_LANGUAGE);
    let n_lines = text.lines().count().max(1);
    let editor = cx.new(|cx| {
        EditorState::new(window, cx)
            .language(language)
            .line_number(true)
            .soft_wrap(false)
            .folding(false)
            .auto_close(false)
            .smart_indent(false)
            .default_value(text)
    });
    TextBody { editor, n_lines, language, size }
}

/// 텍스트 보기 그리기. 읽기 전용이라 키를 쳐도 내용은 바뀌지 않는다.
pub fn text_view(body: &TextBody) -> impl IntoElement {
    div()
        .size_full()
        .pt(zpx(file_view::PADDING_Y))
        .pb(zpx(file_view::PADDING_Y))
        .child(Editor::new(&body.editor).readonly(true).appearance(false).bordered(false).h(gpui_kit::relative(1.0)))
}

/// `line`번째(1부터) 줄이 보이게 스크롤하고 그 줄 앞에 커서를 둔다.
pub fn scroll_to_line(
    body: &TextBody,
    line: usize,
    window: &mut Window,
    cx: &mut Context<WorkWindow>,
) {
    let position = Position::new(u32::try_from(line.saturating_sub(1)).unwrap_or(0), 0);
    body.editor.update(cx, |state, cx| state.set_cursor_position(position, window, cx));
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
