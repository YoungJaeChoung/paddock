//! 작업 폴더가 없을 때 Work 목록 위쪽에 보이는 첫 실행 안내. 문구는 `empty_model`이 정한다.
use gpui_kit::{FontWeight, IntoElement, ParentElement, Styled, div};
use paddock_core::theme::{text, work_empty as size};

use super::empty_model::empty_state as guide_text;
use crate::chrome::zpx;
use crate::look::colors;
use crate::terminal::paint::hsla;

/// 첫 실행 안내를 만든다. 이미 열린 터미널(`has_unassigned`)이 있으면 그 터미널을 쓰라고 안내한다.
pub fn empty_state(has_unassigned: bool) -> impl IntoElement {
    let palette = colors();
    let guide = guide_text(has_unassigned);
    let mut steps = div()
        .flex()
        .flex_col()
        .pl(zpx(size::STEPS_PADDING_LEFT))
        .pr(zpx(size::STEPS_PADDING_RIGHT))
        .pb(zpx(size::STEPS_PADDING_BOTTOM))
        .text_size(zpx(text::SMALL))
        .line_height(gpui_kit::relative(size::STEP_LINE_HEIGHT));
    for (index, step) in guide.steps.iter().enumerate() {
        let mut body = div().flex_1().min_w_0().flex().flex_col().child(step.text);
        if let Some(command) = step.command {
            body = body.child(
                div()
                    .mt(zpx(size::COMMAND_MARGIN_TOP))
                    .font_family(crate::look::terminal_family())
                    .text_color(hsla(palette.fg_strong))
                    .child(command),
            );
        }
        steps = steps.child(
            div()
                .flex()
                .flex_row()
                .py(zpx(size::STEP_PADDING_Y))
                .text_color(hsla(palette.fg))
                .child(
                    div()
                        .flex_none()
                        .w(zpx(size::STEP_NUMBER_WIDTH))
                        .text_color(hsla(palette.fg_muted))
                        .child(format!("{}.", index + 1)),
                )
                .child(body),
        );
    }
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .mt(zpx(size::TITLE_MARGIN_TOP))
                .mx(zpx(size::TITLE_MARGIN_X))
                .mb(zpx(size::TITLE_MARGIN_BOTTOM))
                .text_size(zpx(text::UI))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hsla(palette.fg_strong))
                .child(guide.title),
        )
        .child(
            div()
                .py(zpx(size::TEXT_PADDING_Y))
                .px(zpx(size::TEXT_PADDING_X))
                .text_size(zpx(text::SMALL))
                .text_color(hsla(palette.fg_muted))
                .child(guide.lead),
        )
        .child(steps)
        .child(
            div()
                .py(zpx(size::TEXT_PADDING_Y))
                .px(zpx(size::TEXT_PADDING_X))
                .text_size(zpx(text::SMALL))
                .line_height(gpui_kit::relative(size::NOTE_LINE_HEIGHT))
                .text_color(hsla(palette.fg_muted))
                .child(guide.note),
        )
}
