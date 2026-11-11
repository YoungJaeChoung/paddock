//! 사용량 목록(팝업): 상태 줄 묶음이나 `Usage` 단추를 누르면 오른쪽 아래에 뜨는, 모든 계정의 값을 견주는 창.
//!
//! 도구별 절 안에 행마다 이름·배지(`In status bar`·`Default`)·환경·창별 게이지·`Open in new terminal`이 있고, 계정이 6개 이상이면
//! 검색칸이 생긴다. 열려 있는 동안 모든 계정을 읽는다. 읽기 실패 행이 있으면 `Retry`가 보인다. 무엇을 보일지는 core의 `popup_sections`가 정한다.
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    AnyElement, AppContext, Context, Div, Entity, FontWeight, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use paddock_core::agent_account::Scope;
use paddock_core::theme::{meter, text, usage_popup as size};
use paddock_core::usage_scopes::{
    ACCOUNTS_FAILED, NO_MATCHING_ACCOUNTS, POPUP_HINT, PopupRow, PopupSection, ValueView, popup_sections,
    search_visible,
};

use super::bar::tooltip;
use crate::chrome::zpx;
use crate::look::{colors, terminal_family};
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 열려 있는 목록의 상태.
pub struct PopupState {
    pub search: Entity<InputState>,
    /// 새 터미널을 여는 중인 행의 키.
    pub opening: Option<String>,
    /// 새 터미널을 열지 못한 이유 문장.
    pub error: String,
    /// `Retry`를 눌러 다시 읽는 중이다.
    pub retrying: bool,
}

impl WorkWindow {
    /// 목록을 연다: 모든 계정을 읽기 시작하고 검색칸(있으면)에 포커스를 둔다.
    pub(crate) fn open_usage_popup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.usage_popup.is_none() {
            self.menu = None;
            self.quick = Default::default();
            let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search accounts"));
            cx.subscribe_in(&search, window, |_this, _input, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            self.usage_popup =
                Some(PopupState { search: search.clone(), opening: None, error: String::new(), retrying: false });
            self.usage.refresh(true);
            if search_visible(self.usage.book.profiles().len(), "") {
                search.update(cx, |state, cx| state.focus(window, cx));
            }
            cx.notify();
        }
    }

    /// 목록을 닫고 키 입력을 선택한 터미널로 돌린다.
    pub(crate) fn close_usage_popup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.usage_popup.take().is_some() {
            self.focus_selected(window, cx);
            cx.notify();
        }
    }

    /// 읽기 실패를 다시 읽는다: 진행 중인 응답을 버리고 모든 계정을 새로 읽는다.
    pub(crate) fn usage_popup_retry(&mut self) {
        if let Some(popup) = self.usage_popup.as_mut()
            && !popup.retrying
        {
            popup.retrying = true;
            self.usage.retry();
        }
    }

    /// 행의 계정을 새 터미널로 연다. 열렸으면 목록을 닫고 상태 줄 선택도 그 계정으로 바꾼다. 열지 못하면 목록에 이유를 보인다.
    pub(crate) fn usage_popup_open_account(
        &mut self,
        scope: &Scope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let profile =
            scope.profile.clone().filter(|_| self.usage_popup.as_ref().is_none_or(|popup| popup.opening.is_none()));
        if let Some(profile) = profile {
            self.open_profile_terminal(&profile, window, cx);
        }
    }

    fn open_profile_terminal(
        &mut self,
        profile: &paddock_core::agent_account::AccountProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let place = self.workspace.new_place();
        match self.account_terminal_request(&profile.id, &place) {
            Ok(want) => {
                self.close_usage_popup(window, cx);
                self.open_terminal(want, place, window, cx);
                self.usage.choose(Scope::of_profile(profile));
            }
            Err(_) => {
                if let Some(popup) = self.usage_popup.as_mut() {
                    popup.error = format!(
                        "Could not open {} \u{b7} {}. Try Open in new terminal again.",
                        paddock_core::usage_scopes::provider_title(&profile.provider),
                        profile.label
                    );
                }
                cx.notify();
            }
        }
    }

    /// 목록의 `Manage accounts…`: 목록을 닫고 Accounts 창을 연다.
    pub(crate) fn usage_popup_manage(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_usage_popup(window, cx);
        self.open_accounts(window, cx);
    }

    /// 검색칸의 지금 글자.
    pub(crate) fn usage_popup_query(
        &self,
        cx: &gpui_kit::App,
    ) -> String {
        self.usage_popup.as_ref().map(|popup| popup.search.read(cx).value().to_string()).unwrap_or_default()
    }

    /// 지금 보이는 목록 절들.
    pub(crate) fn usage_popup_sections(
        &self,
        cx: &gpui_kit::App,
    ) -> Vec<PopupSection> {
        let items = self.usage.items;
        popup_sections(
            &self.usage.book,
            &|provider| items.provider(provider),
            &self.usage_popup_query(cx),
            chrono::Utc::now().timestamp(),
        )
    }
}

fn small_button(id: SharedString) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    div()
        .id(id)
        .flex_none()
        .min_h(zpx(size::BUTTON_HEIGHT))
        .px(zpx(size::BUTTON_PADDING_X))
        .flex()
        .items_center()
        .rounded(zpx(size::BUTTON_RADIUS))
        .border_1()
        .border_color(hsla(palette.popup_border))
        .bg(hsla(palette.control_bg))
        .text_color(hsla(palette.fg))
        .text_size(zpx(text::UI))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)))
}

fn hint(text_value: impl Into<SharedString>) -> Div {
    div().text_size(zpx(size::HINT_SIZE)).text_color(hsla(colors().fg_muted)).child(text_value.into())
}

fn badge(
    label: &'static str,
    filled: bool,
) -> Div {
    let palette = colors();
    let base = div()
        .flex_none()
        .px(zpx(size::BADGE_PADDING_X))
        .rounded(zpx(size::BADGE_RADIUS))
        .text_size(zpx(size::BADGE_SIZE))
        .child(label);
    if filled {
        base.bg(hsla(palette.selected_bg)).text_color(hsla(palette.fg_strong))
    } else {
        base.border_1().border_color(hsla(palette.popup_border)).text_color(hsla(palette.fg_muted))
    }
}

/// 게이지 한 줄: 이름표 44 · 막대(남는 폭) · 퍼센트 42.
fn meter_line(view: &paddock_core::usage_scopes::MeterView) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let accent = if view.used >= meter::CRITICAL_PERCENT {
        palette.danger
    } else if view.used >= meter::HIGH_PERCENT {
        palette.warning
    } else {
        palette.meter_fill
    };
    div()
        .id(SharedString::from(format!("usage-popup-meter-{}", view.id)))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(size::VALUES_GAP))
        .child(
            div()
                .flex_none()
                .w(zpx(size::GRID_LABEL_WIDTH))
                .font_family(terminal_family())
                .text_size(zpx(meter::TEXT_SIZE))
                .text_color(hsla(palette.fg_muted))
                .child(view.label.clone()),
        )
        .child(
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .h(zpx(size::GRID_BAR_HEIGHT))
                .rounded(zpx(meter::BAR_RADIUS))
                .overflow_hidden()
                .bg(gpui_kit::rgba((palette.meter_fill << 8) | meter::TRACK_ALPHA))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .h_full()
                        .w(gpui_kit::relative(view.used.clamp(0, 100) as f32 / 100.0))
                        .rounded(zpx(meter::BAR_RADIUS))
                        .bg(hsla(accent)),
                ),
        )
        .child(
            div()
                .flex_none()
                .w(zpx(size::GRID_PERCENT_WIDTH))
                .flex()
                .justify_end()
                .font_family(terminal_family())
                .text_size(zpx(meter::TEXT_SIZE))
                .text_color(hsla(if view.used >= meter::HIGH_PERCENT { accent } else { palette.fg }))
                .child(format!("{}%", view.used)),
        )
        .tooltip(tooltip(view.title.clone()))
}

fn row_view(
    row: &PopupRow,
    index: usize,
    opening: Option<&str>,
    last: bool,
    cx: &mut Context<WorkWindow>,
) -> AnyElement {
    let palette = colors();
    let busy = opening.is_some();
    let is_opening = opening == Some(row.key.as_str());
    let mut title = div().flex().flex_row().items_center().gap(zpx(8.0)).child(
        div()
            .min_w_0()
            .truncate()
            .text_size(zpx(size::NAME_SIZE))
            .font_weight(FontWeight::MEDIUM)
            .text_color(hsla(palette.fg))
            .child(row.name.clone()),
    );
    if row.is_default_cli {
        title = title.child(badge("Default", false));
    }
    if row.in_bar {
        title = title.child(badge("In status bar", true));
    }
    let summary = div().flex_1().min_w_0().flex().flex_col().child(title).child(
        div()
            .mt(zpx(3.0))
            .truncate()
            .text_size(zpx(size::ENVIRONMENT_SIZE))
            .text_color(hsla(palette.fg_muted))
            .child(row.environment.clone()),
    );
    let mut header = div().flex().flex_row().items_center().gap(zpx(12.0)).child(summary);
    if row.can_open {
        let scope = row.scope.clone();
        let mut open = small_button(SharedString::from(format!("usage-popup-open-{index}"))).child(if is_opening {
            "Opening\u{2026}"
        } else {
            "Open in new terminal"
        });
        open = if busy {
            open.opacity(0.5).cursor_default()
        } else {
            open.on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.usage_popup_open_account(&scope, window, cx);
                }),
            )
        };
        header = header.child(open);
    }
    let mut values = div()
        .flex()
        .flex_col()
        .justify_center()
        .min_h(zpx(size::VALUES_MIN_HEIGHT))
        .gap(zpx(size::VALUES_GAP))
        .mt(zpx(2.0));
    match &row.value {
        ValueView::Meters { meters, stale, resets, age } => {
            let mut lines = div().flex().flex_col().gap(zpx(size::VALUES_GAP));
            for view in meters {
                lines = lines.child(meter_line(view));
            }
            if *stale {
                lines = lines.opacity(paddock_core::theme::usage_group::STALE_OPACITY);
            }
            values = values.child(lines);
            if let Some(resets) = resets {
                values = values.child(hint(resets.clone()));
            }
            if let Some(age) = age {
                values = values.child(hint(age.explanation.clone()));
            }
        }
        ValueView::State { text: label, title } => {
            values = values.child(
                div()
                    .text_size(zpx(paddock_core::theme::usage_group::STATE_SIZE))
                    .text_color(hsla(palette.fg_muted))
                    .child(*label),
            );
            if !title.is_empty() && *label != "Loading\u{2026}" {
                values = values.child(hint(title.clone()));
            }
        }
    }
    let mut element = div()
        .id(SharedString::from(format!("usage-popup-row-{index}")))
        .flex()
        .flex_col()
        .gap(zpx(size::ROW_GAP))
        .py(zpx(size::ROW_PADDING_Y))
        .child(header)
        .child(values);
    if !last {
        element = element.border_b_1().border_color(hsla(palette.border_subtle));
    }
    element.into_any_element()
}

/// 열린 목록을 그린다: 바깥을 누르면 닫히는 투명 막과 오른쪽 아래 상자.
pub fn usage_popup_view(
    view: &WorkWindow,
    viewport: (f32, f32),
    cx: &mut Context<WorkWindow>,
) -> Option<AnyElement> {
    let popup = view.usage_popup.as_ref()?;
    let palette = colors();
    let zoom = crate::look::zoom();
    let width = (size::WIDTH * zoom).min(viewport.0 - 2.0 * size::RIGHT_MARGIN * zoom);
    let max_height = (size::MAX_HEIGHT * zoom).min(viewport.1 - size::VIEWPORT_HEIGHT_MARGIN * zoom);
    let sections = view.usage_popup_sections(cx);
    let n_profiles = view.usage.book.profiles().len();
    let query = view.usage_popup_query(cx);
    let show_search = search_visible(n_profiles, &query);
    let show_retry = view.usage.book.retry_visible(&|provider| view.usage.items.provider(provider));
    let notice = if !popup.error.is_empty() {
        popup.error.clone()
    } else if view.usage.book.profiles_failed() {
        ACCOUNTS_FAILED.to_string()
    } else {
        String::new()
    };
    let mut heading_actions = div().flex().flex_row().items_center().gap(zpx(8.0));
    if show_retry {
        heading_actions = heading_actions.child(
            small_button("usage-popup-retry".into())
                .child(if popup.retrying { "Retrying\u{2026}" } else { "Retry" })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, _window, cx| {
                        cx.stop_propagation();
                        this.usage_popup_retry();
                        cx.notify();
                    }),
                ),
        );
    }
    heading_actions = heading_actions.child(small_button("usage-popup-close".into()).child("Close").on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            this.close_usage_popup(window, cx);
        }),
    ));
    let mut rows = div()
        .id("usage-popup-rows")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(zpx(size::SECTION_GAP));
    let mut index = 0;
    for section in &sections {
        let mut block = div()
            .flex()
            .flex_col()
            .px(zpx(size::SECTION_PADDING_X))
            .py(zpx(size::SECTION_PADDING_Y))
            .rounded(zpx(size::SECTION_RADIUS))
            .border_1()
            .border_color(hsla(palette.popup_border))
            .bg(hsla(palette.control_bg))
            .child(
                div()
                    .py(zpx(4.0))
                    .text_size(zpx(size::SECTION_TITLE_SIZE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(hsla(palette.fg_strong))
                    .child(section.title),
            );
        for (position, row) in section.rows.iter().enumerate() {
            index += 1;
            block = block.child(row_view(row, index, popup.opening.as_deref(), position + 1 == section.rows.len(), cx));
        }
        rows = rows.child(block);
    }
    if sections.is_empty() {
        rows = rows.child(hint(NO_MATCHING_ACCOUNTS));
    }
    let mut body = div()
        .id("usage-popup")
        .absolute()
        .right(zpx(size::RIGHT_MARGIN))
        .bottom(zpx(size::BOTTOM_MARGIN))
        .w(px(width))
        .max_h(px(max_height))
        .p(zpx(size::PADDING))
        .flex()
        .flex_col()
        .gap(zpx(size::GAP))
        .rounded(zpx(size::RADIUS))
        .border_1()
        .border_color(hsla(palette.popup_border))
        .bg(hsla(palette.popup_bg))
        .text_color(hsla(palette.fg))
        .text_size(zpx(text::UI))
        .shadow_lg()
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_this, _event: &MouseDownEvent, _window, cx| cx.stop_propagation()),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(zpx(12.0))
                .child(div().text_size(zpx(size::TITLE_SIZE)).font_weight(FontWeight::SEMIBOLD).child("Account usage"))
                .child(heading_actions),
        )
        .child(hint(POPUP_HINT));
    if show_search {
        body = body.child(
            div()
                .h(zpx(size::SEARCH_HEIGHT))
                .rounded(zpx(size::BUTTON_RADIUS))
                .border_1()
                .border_color(hsla(palette.popup_border))
                .bg(hsla(palette.control_bg))
                .child(Input::new(&popup.search).appearance(false)),
        );
    }
    if !notice.is_empty() {
        body = body.child(div().text_size(zpx(size::HINT_SIZE)).text_color(hsla(palette.danger)).child(notice));
    }
    body = body.child(rows).child(div().flex().flex_row().justify_end().pt(zpx(4.0)).child(
        small_button("usage-popup-manage".into()).child("Manage accounts\u{2026}").on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.usage_popup_manage(window, cx);
            }),
        ),
    ));
    Some(
        div()
            .id("usage-popup-scrim")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseDownEvent, window, cx| this.close_usage_popup(window, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _event: &MouseDownEvent, window, cx| this.close_usage_popup(window, cx)),
            )
            .child(body)
            .into_any_element(),
    )
}
