//! 상태 줄 오른쪽의 사용량 묶음(Claude·Codex)과, 폭이 모자랄 때 그 전부를 대신하는 `Usage` 단추를 그린다.
//!
//! 무엇을 보일지(값·문구·접을지)는 core의 `usage_scopes`가 정하고, 여기는 그 결과를 그린다.
//! 묶음 하나는 앞 구분선·출처 표지·도구 이름·계정 이름과 창별 게이지(`5h ▰▰▱▱ 23%`)다. 값을 못 보일 때는 그 이유 글(`Loading…` 등)이 대신한다.
use gpui_kit::{
    AnyView, AppContext, Context, Div, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use paddock_core::theme::{memory_notice, meter, text, usage_compact, usage_group};
use paddock_core::usage_scopes::{BarMode, GroupView, MeterView, ValueView};

use crate::chrome::zpx;
use crate::look::{colors, terminal_family};
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 마우스를 올리면 뜨는 말풍선.
struct Tip {
    text: SharedString,
}

impl Render for Tip {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = colors();
        div()
            .max_w(zpx(360.0))
            .px(zpx(8.0))
            .py(zpx(5.0))
            .rounded(zpx(4.0))
            .border_1()
            .border_color(hsla(palette.popup_border))
            .bg(hsla(palette.popup_bg))
            .text_color(hsla(palette.fg))
            .text_size(zpx(text::SMALL))
            .child(self.text.clone())
    }
}

/// 말풍선을 붙이는 처리기.
pub(crate) fn tooltip(text: String) -> impl Fn(&mut Window, &mut gpui_kit::App) -> AnyView + 'static {
    let text = SharedString::from(text);
    move |_window, cx| cx.new(|_| Tip { text: text.clone() }).into()
}

/// 묶음·`Usage` 단추를 누르면 사용량 목록을 연다.
fn open_listener(cx: &mut Context<WorkWindow>) -> impl Fn(&MouseDownEvent, &mut Window, &mut gpui_kit::App) + 'static {
    cx.listener(|this, _event: &MouseDownEvent, window, cx| {
        cx.stop_propagation();
        this.open_usage_popup(window, cx);
    })
}

/// 게이지 하나: 이름표 + 막대 + 퍼센트. 75% 이상은 경고색, 90% 이상은 위험색이다.
pub fn meter_view(
    view: &MeterView,
    memory_bar: bool,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let percent = view.used;
    let accent = if percent >= meter::CRITICAL_PERCENT {
        palette.danger
    } else if percent >= meter::HIGH_PERCENT {
        palette.warning
    } else {
        palette.meter_fill
    };
    let width = if memory_bar { meter::MEMORY_BAR_WIDTH } else { meter::BAR_WIDTH };
    let fill_width = width * (percent.clamp(0, 100) as f32) / 100.0;
    let percent_color = if percent >= meter::HIGH_PERCENT { accent } else { palette.status_bar_fg };
    div()
        .id(SharedString::from(format!("usage-meter-{}", view.id)))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(meter::GAP))
        .child(
            div()
                .font_family(terminal_family())
                .text_size(zpx(meter::TEXT_SIZE))
                .text_color(hsla(palette.fg_muted))
                .child(view.label.clone()),
        )
        .child(
            div()
                .relative()
                .flex_none()
                .w(zpx(width))
                .h(zpx(meter::BAR_HEIGHT))
                .rounded(zpx(meter::BAR_RADIUS))
                .overflow_hidden()
                .bg(gpui_kit::rgba((palette.meter_fill << 8) | meter::TRACK_ALPHA))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .h_full()
                        .w(zpx(fill_width))
                        .rounded(zpx(meter::BAR_RADIUS))
                        .bg(hsla(accent)),
                ),
        )
        .child(
            div()
                .font_family(terminal_family())
                .text_size(zpx(meter::TEXT_SIZE))
                .text_color(hsla(percent_color))
                .child(format!("{percent}%")),
        )
        .tooltip(tooltip(view.title.clone()))
}

/// 묶음 하나. 맨 앞 세로 구분선과 출처 표지·도구 이름·계정 이름을 두고 값(게이지 또는 이유 글)이 이어진다.
fn group_element(
    group: &GroupView,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let is_claude = group.provider == "claude";
    let mut mark = div()
        .text_size(zpx(if is_claude { usage_group::CLAUDE_MARK_SIZE } else { usage_group::MARK_SIZE }))
        .text_color(hsla(palette.fg_muted))
        .child(group.mark);
    if is_claude {
        mark = mark.font_weight(FontWeight::BOLD);
    }
    let mut element = div()
        .id(SharedString::from(format!("usage-group-{}", group.provider)))
        .relative()
        .flex_none()
        .h_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(usage_group::GAP))
        .pl(zpx(usage_group::PADDING_LEFT))
        .pr(zpx(usage_group::PADDING_RIGHT))
        .text_size(zpx(usage_group::NAME_SIZE))
        .cursor_pointer()
        .hover(|style| style.bg(gpui_kit::rgba(colors().status_hover_bg)))
        .on_mouse_down(MouseButton::Left, open_listener(cx))
        .child(
            div()
                .absolute()
                .left_0()
                .top_1_2()
                .mt(zpx(-usage_group::DIVIDER_HEIGHT / 2.0))
                .w(px(1.0))
                .h(zpx(usage_group::DIVIDER_HEIGHT))
                .bg(hsla(palette.border)),
        )
        .child(mark)
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hsla(palette.status_bar_fg))
                .child(group.title),
        )
        .child(
            div()
                .max_w(zpx(usage_group::ACCOUNT_NAME_MAX_WIDTH))
                .truncate()
                .text_color(hsla(palette.fg_muted))
                .child(group.name.clone()),
        );
    element = match &group.value {
        ValueView::Meters { meters, stale, age, .. } => {
            let mut values = div().flex().flex_row().items_center().gap(zpx(usage_group::FIRST_METER_MARGIN));
            for meter_view_data in meters {
                values = values.child(meter_view(meter_view_data, false));
            }
            if *stale {
                values = values.opacity(usage_group::STALE_OPACITY);
            }
            let mut wrapper = div().flex().flex_row().items_center().gap(zpx(usage_group::GAP)).child(values);
            if let Some(age) = age {
                wrapper = wrapper.child(
                    div()
                        .id(SharedString::from(format!("usage-age-{}", group.provider)))
                        .flex_none()
                        .text_size(zpx(usage_group::AGE_SIZE))
                        .text_color(hsla(palette.fg_muted))
                        .child(age.short.clone())
                        .tooltip(tooltip(age.explanation.clone())),
                );
            }
            element.child(wrapper)
        }
        ValueView::State { text, title } => element.child(
            div()
                .id(SharedString::from(format!("usage-state-{}", group.provider)))
                .flex_none()
                .text_size(zpx(usage_group::STATE_SIZE))
                .text_color(hsla(palette.fg_muted))
                .child(*text)
                .tooltip(tooltip(title.clone())),
        ),
    };
    element
}

/// 상태 줄 `Memory` 묶음: 앞 구분선, 이름(`Memory`, 표지 없음), 막대 36 + 퍼센트. 값을 모르면 `—`다. 누를 수 없는 표시만 둔다.
pub fn memory_group(gauge: Option<&super::memory::Gauge>) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let percent = gauge.and_then(|gauge| gauge.percent);
    let tip = match gauge.and_then(|gauge| gauge.details.as_ref()) {
        Some(details) => format!("{} Hide it in Quick settings.", details.summary),
        None => "System memory in use. Hide it in Quick settings.".to_string(),
    };
    let value = match percent {
        Some(percent) => meter_view(
            &MeterView { id: "memory".to_string(), label: String::new(), used: i64::from(percent), title: tip.clone() },
            true,
        )
        .into_any_element(),
        None => div()
            .id("usage-meter-memory")
            .flex()
            .flex_row()
            .items_center()
            .gap(zpx(meter::GAP))
            .child(
                div()
                    .flex_none()
                    .w(zpx(meter::MEMORY_BAR_WIDTH))
                    .h(zpx(meter::BAR_HEIGHT))
                    .rounded(zpx(meter::BAR_RADIUS))
                    .bg(gpui_kit::rgba((palette.meter_fill << 8) | meter::TRACK_ALPHA)),
            )
            .child(
                div()
                    .font_family(terminal_family())
                    .text_size(zpx(meter::TEXT_SIZE))
                    .text_color(hsla(palette.fg_muted))
                    .child("\u{2014}"),
            )
            .tooltip(tooltip(tip))
            .into_any_element(),
    };
    div()
        .id("usage-group-memory")
        .relative()
        .flex_none()
        .h_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(usage_group::GAP))
        .pl(zpx(usage_group::PADDING_LEFT))
        .pr(zpx(usage_group::PADDING_RIGHT))
        .text_size(zpx(usage_group::NAME_SIZE))
        .child(
            div()
                .absolute()
                .left_0()
                .top_1_2()
                .mt(zpx(-usage_group::DIVIDER_HEIGHT / 2.0))
                .w(px(1.0))
                .h(zpx(usage_group::DIVIDER_HEIGHT))
                .bg(hsla(palette.border)),
        )
        .child(div().flex_none().text_color(hsla(palette.fg_muted)).child("Memory"))
        .child(div().ml(zpx(usage_group::AFTER_NAME_METER_MARGIN)).child(value))
}

/// `Memory` 묶음이 차지할 폭의 추정(접을지 정할 때 오른쪽 다른 항목에 더한다).
pub fn memory_group_width() -> f32 {
    usage_group::PADDING_LEFT
        + usage_group::PADDING_RIGHT
        + "Memory".chars().count() as f32 * usage_compact::CHAR_WIDTH
        + usage_group::AFTER_NAME_METER_MARGIN
        + meter::MEMORY_BAR_WIDTH
        + 2.0 * meter::GAP
        + 4.0 * usage_compact::MONO_CHAR_WIDTH
}

/// 메모리 90% 경고: 창 오른쪽 아래의 알림. 경고색 테두리, `Go to <이름>`(가장 큰 터미널이 이 창에 있을 때만)과 닫기.
pub fn memory_notice_view(
    notice: &super::memory::Notice,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::AnyElement {
    let palette = colors();
    let mut actions = div().flex().flex_row().items_center().justify_end().gap(zpx(memory_notice::GAP));
    if let Some((id, label)) = notice.target.clone() {
        actions = actions.child(
            div()
                .id("memory-notice-go")
                .flex_none()
                .h(zpx(memory_notice::BUTTON_HEIGHT))
                .px(zpx(memory_notice::BUTTON_PADDING_X))
                .flex()
                .items_center()
                .rounded(zpx(memory_notice::BUTTON_RADIUS))
                .bg(hsla(palette.button_primary_bg))
                .text_color(hsla(palette.button_primary_fg))
                .text_size(zpx(text::SMALL))
                .cursor_pointer()
                .child(label)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.memory_go_to(id, window, cx);
                    }),
                ),
        );
    }
    div()
        .id("memory-notice")
        .absolute()
        .right(zpx(memory_notice::RIGHT_MARGIN))
        .bottom(zpx(memory_notice::BOTTOM_MARGIN))
        .w(zpx(memory_notice::WIDTH))
        .p(zpx(memory_notice::PADDING))
        .flex()
        .flex_col()
        .gap(zpx(memory_notice::GAP))
        .rounded(zpx(memory_notice::RADIUS))
        .border_1()
        .border_color(hsla(palette.warning))
        .bg(hsla(palette.popup_bg))
        .text_color(hsla(palette.fg))
        .text_size(zpx(memory_notice::TEXT_SIZE))
        .shadow_lg()
        .occlude()
        .child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(zpx(memory_notice::GAP))
                .child(div().flex_none().text_color(hsla(palette.warning)).child(memory_notice::ICON))
                .child(div().flex_1().min_w_0().child(notice.message.clone()))
                .child(
                    div()
                        .id("memory-notice-close")
                        .flex_none()
                        .cursor_pointer()
                        .text_color(hsla(palette.fg_muted))
                        .child(memory_notice::CLOSE)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _event: &MouseDownEvent, _window, cx| {
                                cx.stop_propagation();
                                this.memory.dismiss();
                                cx.notify();
                            }),
                        ),
                ),
        )
        .child(actions)
        .into_any_element()
}

/// 상태 줄 오른쪽에서 사용량 묶음 말고 자리를 쓰는 것(검토 메모 수·저장 표시)의 폭 추정.
pub fn other_right_width(
    review_notes: Option<&str>,
    save: Option<paddock_core::unsaved::SaveStatus>,
) -> f32 {
    use paddock_core::theme::editor;
    use paddock_core::unsaved::{STATUS_SAVED, STATUS_UNSAVED, SaveStatus};
    let chars = |text: &str| text.chars().count() as f32 * usage_compact::CHAR_WIDTH;
    let notes = review_notes.map_or(0.0, |text| chars(text) + editor::STATUS_GAP);
    let saved = match save {
        Some(SaveStatus::Unsaved) => {
            chars(STATUS_UNSAVED)
                + editor::STATUS_GAP
                + 2.0 * editor::BUTTON_PADDING_X
                + chars("Save")
                + editor::STATUS_GAP
        }
        Some(SaveStatus::Saved) => chars(STATUS_SAVED) + editor::STATUS_GAP,
        None => 0.0,
    };
    notes + saved
}

/// 좁은 폭에서 묶음 전부를 대신하는 `Usage` 단추.
fn compact_button(cx: &mut Context<WorkWindow>) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    div()
        .id("usage-compact")
        .relative()
        .flex_none()
        .h_full()
        .flex()
        .items_center()
        .px(zpx(usage_compact::PADDING_X))
        .cursor_pointer()
        .hover(|style| style.bg(gpui_kit::rgba(colors().status_hover_bg)))
        .on_mouse_down(MouseButton::Left, open_listener(cx))
        .text_color(hsla(palette.status_bar_fg))
        .child(usage_compact::LABEL)
        .tooltip(tooltip("Show usage for all accounts.".to_string()))
}

/// 상태 줄 오른쪽에 놓을 사용량 요소들. `mode`가 `Compact`이거나 고른 범위가 하나도 없으면 `Usage` 단추 하나이고,
/// 아니면 켜진 도구의 묶음들이다. 사용량 항목이 모두 꺼져 있으면 아무것도 없다.
pub fn usage_elements(
    view: &WorkWindow,
    groups: &[GroupView],
    mode: BarMode,
    cx: &mut Context<WorkWindow>,
) -> Vec<gpui_kit::AnyElement> {
    let any_enabled = view.usage_items().claude || view.usage_items().codex;
    if paddock_core::usage_scopes::compact_button_visible(mode, groups, any_enabled) {
        vec![compact_button(cx).into_any_element()]
    } else {
        groups.iter().map(|group| group_element(group, cx).into_any_element()).collect()
    }
}
