//! 빠른 열기 동작: 열기·닫기, 색인 결과 따라가기, 질의 바뀜, 강조 이동, 고른 파일 열기.
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{AppContext, Context, Window};
use paddock_core::file_tree::{child_path, hidden_names};
use paddock_core::menu_nav::NavKey;
use paddock_core::quick_open::PLACEHOLDER;
use paddock_core::search_model::NO_FOLDER_NOTICE;

use super::QuickOpenState;
use super::index::IndexJob;
use crate::settings::Settings;
use crate::window_view::{Overlay, WorkWindow};

/// 색인이 도는 동안 결과를 창으로 옮기는 간격.
const PUMP_INTERVAL: Duration = Duration::from_millis(50);

impl WorkWindow {
    /// Ctrl+Shift+P: 빠른 열기를 연다. 이미 열려 있으면 닫는다. 훑을 폴더가 없으면(홈 폴더·파일 시스템 루트의 터미널) 열지 않고 상태 줄에 알린다.
    pub(crate) fn toggle_quick_open(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.overlay == Overlay::QuickOpen {
            self.cancel_overlay(window, cx);
        } else if let Some(folder) = self.search_scope_now() {
            self.settle_composition(window, cx);
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(PLACEHOLDER));
            cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } if this.quick_input_is(input) => this.quick_open_choose(window, cx),
                InputEvent::Change => this.quick_open_refresh(cx),
                _ => {}
            })
            .detach();
            let job = IndexJob::start(folder.clone(), hidden_names(Settings::load().values()));
            self.quick_open = Some(QuickOpenState::new(folder, input.clone(), job));
            self.open_overlay(Overlay::QuickOpen, window, cx);
            input.update(cx, |state, cx| state.focus(window, cx));
            self.watch_quick_open(window, cx);
        } else {
            self.status = Some(NO_FOLDER_NOTICE.to_string());
            cx.notify();
        }
    }

    /// 이 입력칸이 지금 열린 빠른 열기의 것인가(닫힌 창의 늦은 사건을 거른다).
    fn quick_input_is(
        &self,
        input: &gpui_kit::Entity<InputState>,
    ) -> bool {
        self.quick_open.as_ref().is_some_and(|quick| &quick.input == input)
    }

    /// 입력칸의 글.
    fn quick_query(
        &self,
        cx: &gpui_kit::App,
    ) -> String {
        self.quick_open.as_ref().map(|quick| quick.input.read(cx).value().to_string()).unwrap_or_default()
    }

    /// 색인이 도는 동안 결과를 창으로 옮긴다. 창이 닫히거나 색인이 끝나면 이 반복은 끝난다.
    fn watch_quick_open(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let mut running = true;
            while running {
                cx.background_executor().timer(PUMP_INTERVAL).await;
                running = this
                    .update_in(cx, |this, _window, cx| {
                        this.quick_open_refresh(cx);
                        this.quick_open.as_ref().is_some_and(|quick| quick.indexing)
                    })
                    .unwrap_or(false);
            }
        })
        .detach();
    }

    /// 색인 소식과 질의를 반영해 목록을 다시 만든다.
    pub(crate) fn quick_open_refresh(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let query = self.quick_query(cx);
        if self.quick_open.as_mut().is_some_and(|quick| quick.pump(&query)) {
            cx.notify();
        }
    }

    /// ↑·↓·Home·End: 강조를 옮기고 그 줄이 보이게 스크롤한다.
    pub(crate) fn quick_open_step(
        &mut self,
        key: NavKey,
        cx: &mut Context<Self>,
    ) {
        if let Some(quick) = self.quick_open.as_mut() {
            quick.step(key);
            if let Some(at) = quick.nav.highlight {
                quick.scroll.scroll_to_item(at);
            }
        }
        cx.notify();
    }

    /// 마우스를 올린 줄로 강조를 옮긴다.
    pub(crate) fn quick_open_hover(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(quick) = self.quick_open.as_mut()
            && quick.nav.highlight != Some(index)
        {
            quick.nav.hover(index);
            cx.notify();
        }
    }

    /// Enter·줄 누름: 강조한 파일(`index`가 있으면 그 줄)을 파일 탭으로 열고 빠른 열기를 닫는다. 고를 줄이 없으면 그대로 둔다.
    pub(crate) fn quick_open_pick(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.quick_open.as_mut().and_then(|quick| {
            if let Some(index) = index {
                quick.nav.hover(index);
            }
            quick.chosen().map(|relative| {
                relative.split('/').fold(quick.folder.clone(), |parent, name| child_path(&parent, name))
            })
        });
        if let Some(path) = target {
            self.cancel_overlay(window, cx);
            self.open_file(&path, window, cx);
        }
    }

    /// Enter: 강조한 파일을 연다.
    pub(crate) fn quick_open_choose(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.quick_open_pick(None, window, cx);
    }
}
