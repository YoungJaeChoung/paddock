//! 빠른 설정 패널을 그린다: 창 전체를 덮는 투명 막(바깥을 누르면 닫힘)과 보기 줄 톱니 아래의 설정 판.
//! 줄 구성·선택지·놓일 자리는 `model`이 정하고, 이 파일은 그것을 그리고 누름을 창 동작으로 잇기만 한다.
use gpui_kit::{
    Context, Div, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};
use paddock_core::settings_text::{profile_of_shell, step_zoom_level};
use paddock_core::theme::{quick_settings as size, zoom_factor};

use super::model::{
    Choice, Field, INTERFACE_FONT_CANDIDATES, Row, TERMINAL_FONT_CANDIDATES, choices, current_value, font_options,
    panel_rect,
};
use crate::chrome::{checkbox, zpx};
use crate::look::colors;
use crate::settings::Settings;
use crate::shell_choice::{
    available_environments_with, default_shell_with, machine_default_distribution, profile_name_of,
};
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 글꼴 선택상자에서 설정이 비어 있다는 뜻(창 틀 글꼴)의 선택지 이름.
const SYSTEM_FONT_LABEL: &str = "System default";

/// `field`를 고를 수 있는 선택지. 글꼴은 이 컴퓨터에 설치된 것(`installed`)만이고, 기본 셸은 열 수 있는 실행 환경이다.
fn field_choices(
    field: Field,
    settings: &Settings,
    installed: &[String],
) -> Vec<Choice> {
    let values = settings.values();
    let fonts = |candidates: &[&str]| {
        let current = current_value(field, values);
        font_options(installed, candidates, Some(current.as_str()))
    };
    match field {
        Field::TerminalFont => fonts(&TERMINAL_FONT_CANDIDATES)
            .into_iter()
            .map(|name| Choice { value: name.clone(), label: name })
            .collect(),
        Field::InterfaceFont => {
            let system = Choice { value: String::new(), label: SYSTEM_FONT_LABEL.to_string() };
            std::iter::once(system)
                .chain(
                    fonts(&INTERFACE_FONT_CANDIDATES)
                        .into_iter()
                        .map(|name| Choice { value: name.clone(), label: name }),
                )
                .collect()
        }
        Field::DefaultShell => available_environments_with(settings)
            .into_iter()
            .map(|environment| Choice {
                value: profile_of_shell(
                    environment.shell,
                    environment.wsl_distribution.as_deref(),
                    machine_default_distribution().map(String::as_str),
                ),
                label: environment.label,
            })
            .collect(),
        Field::Theme | Field::OnQuit | Field::RestoreOnRestart | Field::AgentPermissions => choices(field),
    }
}

/// 선택상자의 지금 값. 기본 셸은 설정이 없어도 실제로 쓰이는 셸이다.
fn field_current(
    field: Field,
    settings: &Settings,
) -> String {
    match field {
        Field::DefaultShell => {
            profile_name_of(&default_shell_with(settings), machine_default_distribution().map(String::as_str))
        }
        _ => current_value(field, settings.values()),
    }
}

/// 줄 이름과 그 아래 적용 대상 안내.
fn label_cell(row: Row) -> Div {
    let palette = colors();
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(div().text_size(zpx(size::LABEL_SIZE)).text_color(hsla(palette.fg)).child(row.label()))
        .child(div().text_size(zpx(size::HINT_SIZE)).text_color(hsla(palette.fg_muted)).child(row.applies_to()))
}

/// 줄 하나의 바깥 상자: 이름 칸과 오른쪽 조작 칸.
fn row_box() -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(size::ROW_GAP))
        .min_h(zpx(size::ROW_MIN_HEIGHT))
        .py(zpx(size::ROW_PADDING_Y))
}

/// 28x28 ± 단추. 끝에 닿아 더 못 바꾸면 `disabled`로 흐리게 하고 눌러도 아무 일도 없다.
fn step_button(
    id: &'static str,
    glyph: &'static str,
    disabled: bool,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let button = div()
        .id(id)
        .flex_none()
        .w(zpx(size::STEP_SIZE))
        .h(zpx(size::STEP_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(size::STEP_RADIUS))
        .border_1()
        .border_color(hsla(palette.control_border))
        .bg(hsla(palette.control_bg))
        .text_color(hsla(palette.fg))
        .child(glyph);
    if disabled {
        button.opacity(0.4)
    } else {
        button.cursor_pointer().hover(|style| style.bg(hsla(colors().selected_bg)))
    }
}

/// ± 단추를 누를 때의 처리: 끝에 닿았으면(`enabled`가 거짓) 아무것도 바꾸지 않는다.
fn step_listener(
    row: Row,
    delta: i32,
    enabled: bool,
    cx: &mut Context<WorkWindow>,
) -> impl Fn(&MouseDownEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static {
    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
        cx.stop_propagation();
        if enabled {
            this.quick_step(row, delta, window, cx);
        }
    })
}

/// − 값 + 줄(배율·글꼴 크기).
fn stepper_row(
    row: Row,
    value: String,
    can_decrease: bool,
    can_increase: bool,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let (less_id, more_id) = match row {
        Row::Zoom => ("quick-zoom-less", "quick-zoom-more"),
        _ => ("quick-size-less", "quick-size-more"),
    };
    row_box()
        .child(label_cell(row))
        .child(
            step_button(less_id, "\u{2212}", !can_decrease)
                .on_mouse_down(MouseButton::Left, step_listener(row, -1, can_decrease, cx)),
        )
        .child(
            div()
                .min_w(zpx(size::VALUE_MIN_WIDTH))
                .flex()
                .justify_center()
                .text_size(zpx(size::VALUE_SIZE))
                .text_color(hsla(palette.fg_strong))
                .font_family(crate::look::terminal_family())
                .child(value),
        )
        .child(
            step_button(more_id, "+", !can_increase)
                .on_mouse_down(MouseButton::Left, step_listener(row, 1, can_increase, cx)),
        )
}

/// 선택상자 줄. 누르면 아래에 선택지가 펼쳐진다.
fn select_row(
    field: Field,
    row: Row,
    shown: String,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let select = div()
        .id(("quick-select", field as usize))
        .flex_none()
        .w(zpx(size::SELECT_WIDTH))
        .h(zpx(size::SELECT_HEIGHT))
        .px(zpx(size::SELECT_PADDING_X))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(size::SELECT_CONTENT_GAP))
        .rounded(zpx(size::SELECT_RADIUS))
        .border_1()
        .border_color(hsla(palette.control_border))
        .bg(hsla(palette.control_bg))
        .text_size(zpx(size::LABEL_SIZE))
        .text_color(hsla(palette.fg_strong))
        .cursor_pointer()
        .child(div().flex_1().min_w_0().truncate().child(SharedString::from(shown)))
        .child(div().flex_none().text_color(hsla(palette.fg_muted)).child("\u{25be}"))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, _window, cx| {
                cx.stop_propagation();
                this.quick_expand(field, cx);
            }),
        );
    row_box().child(label_cell(row)).child(select)
}

/// 펼친 선택지 목록. 지금 값에 체크 표시가 붙는다.
fn options_list(
    field: Field,
    options: Vec<Choice>,
    current: &str,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let mut list = div()
        .flex()
        .flex_col()
        .mb(zpx(size::OPTION_LIST_MARGIN_BOTTOM))
        .rounded(zpx(size::SELECT_RADIUS))
        .border_1()
        .border_color(hsla(palette.control_border))
        .bg(hsla(palette.control_bg));
    for (index, option) in options.into_iter().enumerate() {
        let chosen = option.value == current;
        let value = option.value.clone();
        list = list.child(
            div()
                .id(("quick-option", field as usize * 100 + index))
                .h(zpx(size::OPTION_HEIGHT))
                .px(zpx(size::SELECT_PADDING_X * 2.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(zpx(size::OPTION_GAP))
                .text_size(zpx(size::LABEL_SIZE))
                .text_color(hsla(if chosen { palette.accent } else { palette.fg_strong }))
                .cursor_pointer()
                .hover(|style| style.bg(hsla(colors().selected_bg)))
                .child(div().flex_none().w(zpx(size::OPTION_CHECK_WIDTH)).child(if chosen { "\u{2713}" } else { "" }))
                .child(div().flex_1().min_w_0().truncate().child(SharedString::from(option.label)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.quick_choose(field, value.clone(), window, cx);
                    }),
                ),
        );
    }
    list
}

/// 소리 켬/끔 줄.
fn sound_row(
    on: bool,
    cx: &mut Context<WorkWindow>,
) -> Div {
    row_box().child(label_cell(Row::DoneSound)).child(
        div()
            .id("quick-sound")
            .flex_none()
            .p(zpx(size::SOUND_HIT_PADDING))
            .cursor_pointer()
            .child(checkbox(on))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.quick_toggle_sound(window, cx);
                }),
            ),
    )
}

/// 마지막 줄: 설정과 무관하게 이번만 모든 터미널을 끝내고 앱을 닫는다.
fn quit_row(cx: &mut Context<WorkWindow>) -> impl IntoElement {
    let palette = colors();
    div()
        .mt(zpx(size::LAST_ROW_MARGIN_TOP))
        .pt(zpx(size::LAST_ROW_PADDING_TOP))
        .border_t_1()
        .border_color(hsla(palette.border_subtle))
        .child(
            div()
                .id("quick-quit-stop")
                .px(zpx(size::LAST_ROW_PADDING_X))
                .pt(zpx(size::LAST_ROW_BUTTON_PADDING_TOP))
                .pb(zpx(size::LAST_ROW_PADDING_BOTTOM))
                .rounded(zpx(size::LAST_ROW_RADIUS))
                .flex()
                .flex_col()
                .cursor_pointer()
                .hover(|style| style.bg(hsla(colors().selected_bg)))
                .child(
                    div()
                        .text_size(zpx(size::LABEL_SIZE))
                        .text_color(hsla(palette.danger))
                        .child(Row::QuitAndStopAll.label()),
                )
                .child(
                    div()
                        .text_size(zpx(size::HINT_SIZE))
                        .text_color(hsla(palette.fg_muted))
                        .child(Row::QuitAndStopAll.applies_to()),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.quit_and_stop_all(window, cx);
                    }),
                ),
        )
}

/// 열린 빠른 설정 패널을 그린다.
pub fn quick_panel(
    window_view: &WorkWindow,
    viewport: (f32, f32),
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    // 패널을 열 때(그리고 값을 바꿀 때) 읽어 둔 설정이다. 그릴 때마다 파일을 다시 읽지 않는다.
    let settings = window_view.quick_settings();
    let values = settings.values();
    let appearance = values.appearance();
    let zoom_level = appearance.zoom_level;
    let rect = panel_rect(viewport, crate::look::zoom());
    let expanded = window_view.quick.expanded;
    let installed = window_view.installed_fonts();

    let mut panel = div()
        .id("quick-settings")
        .absolute()
        .left(px(rect.left))
        .top(px(rect.top))
        .w(px(rect.width))
        .max_h(px(rect.max_height))
        .overflow_y_scroll()
        .p(zpx(size::PADDING))
        .flex()
        .flex_col()
        .rounded(zpx(size::RADIUS))
        .border_1()
        .border_color(hsla(palette.popup_border))
        .bg(hsla(palette.popup_bg))
        .text_color(hsla(palette.fg))
        .text_size(zpx(size::FONT_SIZE))
        .shadow_lg()
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_this, _event: &MouseDownEvent, _window, cx| cx.stop_propagation()),
        )
        .child(
            div()
                .mb(zpx(size::TITLE_MARGIN_BOTTOM))
                .text_size(zpx(size::FONT_SIZE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hsla(palette.fg_strong))
                .child("Quick settings"),
        );
    for row in Row::ALL {
        let select_field = match row {
            Row::Theme => Some(Field::Theme),
            Row::TerminalFont => Some(Field::TerminalFont),
            Row::InterfaceFont => Some(Field::InterfaceFont),
            Row::DefaultShell => Some(Field::DefaultShell),
            Row::OnQuit => Some(Field::OnQuit),
            Row::RestoreOnRestart => Some(Field::RestoreOnRestart),
            Row::AgentPermissions => Some(Field::AgentPermissions),
            Row::Zoom | Row::FontSize | Row::DoneSound | Row::QuitAndStopAll => None,
        };
        if let Some(field) = select_field {
            let options = field_choices(field, settings, installed);
            let current = field_current(field, settings);
            let shown = options
                .iter()
                .find(|option| option.value == current)
                .map(|option| option.label.clone())
                .unwrap_or_else(|| if current.is_empty() { SYSTEM_FONT_LABEL.to_string() } else { current.clone() });
            panel = panel.child(select_row(field, row, shown, cx));
            if expanded == Some(field) {
                panel = panel.child(options_list(field, options, &current, cx));
            }
        } else {
            panel = match row {
                Row::Zoom => panel.child(stepper_row(
                    row,
                    format!("{}%", (zoom_factor(zoom_level) * 100.0).round()),
                    step_zoom_level(zoom_level, -1) != zoom_level.round(),
                    step_zoom_level(zoom_level, 1) != zoom_level.round(),
                    cx,
                )),
                Row::FontSize => panel.child(stepper_row(
                    row,
                    format!("{}px", appearance.terminal_font_size),
                    appearance.terminal_font_size > paddock_core::settings_text::limit::FONT_SIZE_MIN,
                    appearance.terminal_font_size < paddock_core::settings_text::limit::FONT_SIZE_MAX,
                    cx,
                )),
                Row::DoneSound => panel.child(sound_row(values.agent_done_sound(), cx)),
                _ => panel.child(quit_row(cx)),
            };
        }
    }
    div()
        .id("quick-settings-scrim")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.close_quick_settings(window, cx)),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.close_quick_settings(window, cx)),
        )
        .child(panel)
}
