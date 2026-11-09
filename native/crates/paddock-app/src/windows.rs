//! 본 창이 분리 창을 여닫는 동작: `Move to New Window`, `Move Back to Main Window`, 분리 창 활성, 종료 때 분리 창 닫기.
//!
//! 분리는 자리(어느 창의 어느 칸)만 옮긴다. 탭·세션·Work 목록의 소속은 그대로 본 창이 쥐고, 터미널 보기(`TerminalView`) 엔티티를
//! 새 창의 뿌리 보기 안에 그린다. 터미널 연결과 수신 스레드는 터미널마다 하나뿐이라 데몬과 프로토콜은 바뀌지 않는다.
use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, Focusable, Pixels, Point, TitlebarOptions, Window,
    WindowBounds, WindowOptions, point, px, size,
};
use paddock_core::tab_drop::DropMode;
use paddock_core::theme::detached_window as detached_size;

use crate::detached_window::{DetachedBody, DetachedShared, DetachedWindow};
use crate::terminal::ime::SentComposition;
use crate::terminal::paint::GlyphCache;
use crate::window_view::WorkWindow;
use crate::work::menu_model::{MenuAction, MenuKind};
use crate::workspace::DockPlace;

/// 전환 중인 탭을 옮기려 할 때 상태 줄에 알리는 문장.
const REFUSED_WHILE_SWITCHING: &str = "Wait for the account switch to finish before moving this tab.";

/// 탭을 새 창으로 옮길 수 없는 사정이 있으면 상태 줄에 알릴 문장. 계정 전환 중인 터미널은 번호가 곧 바뀌므로 옮기지 않는다.
///
/// Examples
/// --------
/// | 전환 중 | 결과                                                                 |
/// |---------|----------------------------------------------------------------------|
/// | 예      | `Some("Wait for the account switch to finish before moving this tab.")` |
/// | 아니오  | `None`                                                               |
fn move_refusal(is_switching: bool) -> Option<&'static str> {
    is_switching.then_some(REFUSED_WHILE_SWITCHING)
}

/// 열려 있는 분리 창 하나: 창 손잡이, 그 창의 뿌리 보기, 본 창이 읽는 겉모습.
pub(crate) struct DetachedHandle {
    pub(crate) window: AnyWindowHandle,
    pub(crate) view: Entity<DetachedWindow>,
    pub(crate) shared: Rc<DetachedShared>,
}

/// 자동 점검이 화면 밖 실행(`--offscreen`)인지. 화면 밖 실행에서는 분리 창도 화면 밖에 활성화 없이 열고, 창 활성화는 요청만 센다.
pub(crate) fn is_offscreen() -> bool {
    #[cfg(feature = "automation")]
    let offscreen = crate::automation::is_offscreen();
    #[cfg(not(feature = "automation"))]
    let offscreen = false;
    offscreen
}

/// 분리 창을 여는 옵션. `origin`은 창 왼쪽 위의 화면 좌표(논리 px)다.
pub(crate) fn detached_options(
    title: &str,
    origin: Point<Pixels>,
) -> WindowOptions {
    let offscreen = is_offscreen();
    #[cfg(feature = "automation")]
    let origin = if offscreen {
        point(px(crate::automation::offscreen_x(detached_size::WIDTH)), px(f32::from(origin.y).max(100.0)))
    } else {
        origin
    };
    // 자동 점검이 크기를 정했으면 그 크기로, 화면 안에서 연다(화면 밖 좌표는 시스템이 기본 크기로 되돌린다).
    #[cfg(feature = "automation")]
    let (origin, width, height) = match crate::automation::detach_size() {
        Some((width, height)) => (point(px(40.0), px(40.0)), width as f32, height as f32),
        None => (origin, detached_size::WIDTH, detached_size::HEIGHT),
    };
    #[cfg(not(feature = "automation"))]
    let (width, height) = (detached_size::WIDTH, detached_size::HEIGHT);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, size(px(width), px(height))))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.to_string().into()),
            appears_transparent: cfg!(windows),
            ..Default::default()
        }),
        focus: !offscreen,
        window_min_size: Some(size(px(detached_size::MIN_WIDTH), px(detached_size::MIN_HEIGHT))),
        ..Default::default()
    }
}

impl WorkWindow {
    /// 터미널 `id`를 탭 하나짜리 새 창으로 옮긴다. `at`은 새 창 왼쪽 위의 화면 좌표이고, 없으면 본 창에서 (40, 40) 떨어진 자리다.
    ///
    /// 이미 다른 창에 있으면 그 창을 앞으로 가져오고, 전환 중인 탭은 옮기지 않는다. 다른 묶음의 탭이면 그 묶음을 먼저 보인다.
    pub(crate) fn move_to_new_window(
        &mut self,
        id: u32,
        at: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.tab(id).is_none() {
            return;
        }
        if let Some(refusal) = move_refusal(self.switching.contains(&id)) {
            self.status = Some(refusal.to_string());
            cx.notify();
        } else if self.workspace.is_detached(id) {
            self.activate_detached(id, cx);
        } else {
            // 숨은 묶음의 탭이면 그 묶음이 먼저 보이게 한다(선택이 다른 묶음에 있으면 옮긴 뒤 선택을 넘길 이웃도 그 묶음에 있어야 한다).
            if !self.workspace.view_ids().contains(&id) {
                self.select_tab(id, window, cx);
            }
            let before = self.snapshot_active();
            if self.workspace.detach_terminal(id).is_ok() {
                self.settle_active(before, false);
                let opened = self.open_detached_window(id, at, window, cx);
                if !opened {
                    // 창을 열지 못했으면 탭을 제자리로 돌려놓는다.
                    self.workspace.reattach_terminal(id, None);
                    self.status = Some("Could not open a new window.".to_string());
                }
                self.focus_selected(window, cx);
                self.persist();
                cx.notify();
            }
        }
    }

    /// 분리 창을 열고 터미널 보기를 그 창으로 옮긴다. 열었으면 true다.
    fn open_detached_window(
        &mut self,
        id: u32,
        at: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let view = self.workspace.tab(id).map(|tab| tab.content.view.clone());
        let mut opened = false;
        if let Some(view) = view {
            let title = self.workspace.name(id).to_string();
            let shared = Rc::new(DetachedShared::new(&title));
            let origin = at.unwrap_or_else(|| {
                let bounds = window.bounds();
                point(
                    bounds.origin.x + px(detached_size::MENU_OFFSET),
                    bounds.origin.y + px(detached_size::MENU_OFFSET),
                )
            });
            let main = cx.entity();
            let main_window = self.main_window;
            let result = gpui_kit::open_window(detached_options(&title, origin), cx, |detached_window, cx| {
                let cache = Rc::new(RefCell::new(GlyphCache::default()));
                view.update(cx, |view, view_cx| {
                    view.rehome(cache, SentComposition::default(), detached_window, view_cx)
                });
                cx.new(|cx| {
                    DetachedWindow::new(
                        main,
                        main_window,
                        DetachedBody::Terminal { id, view: view.clone() },
                        None,
                        None,
                        None,
                        shared.clone(),
                        detached_window,
                        cx,
                    )
                })
            });
            if let Ok((handle, detached_view)) = result {
                #[cfg(feature = "automation")]
                let _ = handle.update(cx, |_, detached_window, _| {
                    crate::platform_keys::note_window_handle(&id.to_string(), detached_window);
                    crate::platform_keys::log_geometry(&format!("detached-{id}"), detached_window);
                });
                #[cfg(feature = "automation")]
                crate::platform_keys::log_geometry("main", window);
                self.detached_windows.insert(id, DetachedHandle { window: handle, view: detached_view, shared });
                opened = true;
            }
        }
        opened
    }

    /// 다른 창에 있던 터미널 `id`를 이 창으로 되돌린다: 칸 배치에 `place`(없으면 첫 칸 끝)로 넣고, 터미널 보기를 이 창의 글리프 캐시와
    /// 조합 기억으로 다시 묶고, 선택해 앞으로 가져온 뒤 분리 창을 닫는다.
    ///
    /// 분리 창을 닫는 길(창 닫기, 메뉴, 끌기)이 모두 이 함수 하나를 지난다.
    pub(crate) fn move_back(
        &mut self,
        id: u32,
        place: Option<DockPlace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.reattach_terminal(id, place) {
            // 이 터미널에 붙어 있던 찾기 띠는 분리 창에 그려지던 것이라 닫는다(입력칸이 그 창에 묶여 있다).
            if self.find.as_ref().is_some_and(|bar| bar.terminal == id) {
                self.find = None;
                if let Some(tab) = self.workspace.tab(id) {
                    tab.content.session.grid.term.lock().selection = None;
                }
            }
            let view = self.workspace.tab(id).map(|tab| tab.content.view.clone());
            if let Some(view) = view {
                let (cache, already_sent) = (self.cache.clone(), self.already_sent.clone());
                view.update(cx, |view, view_cx| view.rehome(cache, already_sent, window, view_cx));
            }
            self.close_detached_window(id, cx);
            self.activate_main(window);
            self.select_tab(id, window, cx);
            cx.notify();
        }
    }

    /// 분리 창에서 온 닫기 요청: 본 창을 앞으로 가져오고 본 창에 닫기 확인을 연다(셸이 끝났으면 바로 닫는다).
    pub(crate) fn request_close_from_detached(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_main(window);
        self.request_close(id, window, cx);
    }

    /// 분리 창 `id`를 앞으로 가져온다. 화면 밖 자동 점검에서는 요청만 센다.
    pub(crate) fn activate_detached(
        &mut self,
        id: u32,
        cx: &mut Context<Self>,
    ) {
        if let Some(detached) = self.detached_windows.get(&id) {
            detached.shared.n_activation_requests.set(detached.shared.n_activation_requests.get() + 1);
            if !is_offscreen() {
                let _ = detached.window.update(cx, |_, detached_window, _| detached_window.activate_window());
            }
        }
    }

    /// 본 창을 앞으로 가져온다. 화면 밖 자동 점검에서는 요청만 센다.
    pub(crate) fn activate_main(
        &mut self,
        window: &mut Window,
    ) {
        self.n_main_activation_requests += 1;
        if !is_offscreen() {
            window.activate_window();
        }
    }

    /// 터미널 `id`의 분리 창이 있으면 닫는다(탭을 닫았거나 되돌렸을 때). 그 창이 지금 갱신 중이면 창이 스스로 닫는다.
    pub(crate) fn close_detached_window(
        &mut self,
        id: u32,
        cx: &mut Context<Self>,
    ) {
        if let Some(detached) = self.detached_windows.remove(&id) {
            let _ = detached.window.update(cx, |_, detached_window, _| detached_window.remove_window());
            #[cfg(feature = "automation")]
            crate::automation::note_window_handle(&id.to_string(), None);
        }
    }

    /// 계정 전환으로 터미널 `old`가 `new`로 바뀌었을 때, `old`가 다른 창에 있었다면 그 창이 `new`를 이어서 보이게 한다.
    /// 창을 닫고 다시 열지 않으므로 창 위치와 크기가 그대로다.
    pub(crate) fn retarget_detached_window(
        &mut self,
        old: u32,
        new: u32,
        cx: &mut Context<Self>,
    ) {
        let view = self.workspace.tab(new).map(|tab| tab.content.view.clone());
        if let (Some(detached), Some(view)) = (self.detached_windows.remove(&old), view) {
            let (view_in_window, entity) = (view.clone(), detached.view.clone());
            let _ = detached.window.update(cx, |_, detached_window, cx| {
                let cache = Rc::new(RefCell::new(GlyphCache::default()));
                view_in_window.update(cx, |view, view_cx| {
                    view.rehome(cache, SentComposition::default(), detached_window, view_cx)
                });
                // 새 보기는 만들 때 본 창에 포커스를 걸었다. 분리 창의 키가 새 터미널로 가도록 이 창의 포커스를 새 보기로 옮긴다.
                detached_window.focus(&view_in_window.focus_handle(cx), cx);
                entity.update(cx, |detached_root, detached_cx| {
                    detached_root.retarget(new, view_in_window.clone(), detached_cx)
                });
            });
            #[cfg(feature = "automation")]
            {
                crate::automation::note_window_handle(&old.to_string(), None);
                let _ = detached.window.update(cx, |_, detached_window, _| {
                    crate::platform_keys::note_window_handle(&new.to_string(), detached_window)
                });
            }
            self.detached_windows.insert(new, detached);
        }
    }

    /// 더는 다른 창에 있지 않은 터미널(쫓겨났거나 닫혔다)의 분리 창을 닫고 목록에서 뺀다.
    pub(crate) fn prune_detached_windows(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let open: Vec<u32> = self.detached_windows.keys().copied().collect();
        for id in self.workspace.stale_detached_ids(&open) {
            self.close_detached_window(id, cx);
        }
    }

    /// 분리 창 `id`의 손잡이.
    #[cfg(feature = "automation")]
    pub(crate) fn detached_window_handle(
        &self,
        id: u32,
    ) -> Option<AnyWindowHandle> {
        self.detached_windows.get(&id).map(|detached| detached.window)
    }

    /// 자동 점검의 창 이름(`keywin`·`textwin`의 첫 낱말)이 가리키는 분리 창의 손잡이: 터미널 번호이거나 파일 분리 창의 `file_<이름>`이다.
    #[cfg(feature = "automation")]
    pub(crate) fn detached_window_handle_named(
        &self,
        name: &str,
    ) -> Option<AnyWindowHandle> {
        match name.parse::<u32>() {
            Ok(id) => self.detached_window_handle(id),
            Err(_) => self
                .detached_file_key_named(name)
                .and_then(|key| self.detached_file_windows.get(&key))
                .map(|detached| detached.window),
        }
    }

    /// 자동 점검: 분리 창 `id`에 사람이 창 X를 누른 것과 같은 닫기 메시지를 보낸다(분리 창이 탭을 되돌리고 닫힌다).
    #[cfg(feature = "automation")]
    pub(crate) fn post_close_to_detached(
        &mut self,
        id: u32,
        cx: &mut Context<Self>,
    ) {
        if let Some(detached) = self.detached_windows.get(&id) {
            let _ = detached
                .window
                .update(cx, |_, detached_window, _| crate::platform_keys::post_window_close(detached_window));
        }
    }

    /// 자동 점검: 분리 창 `id`의 크기를 바꾼다(논리 px).
    #[cfg(feature = "automation")]
    pub(crate) fn resize_detached(
        &mut self,
        id: u32,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) {
        if let Some(detached) = self.detached_windows.get(&id) {
            let _ =
                detached.window.update(cx, |_, detached_window, _| detached_window.resize(size(px(width), px(height))));
        }
    }

    /// 자동 점검: 분리 창 `id`의 탭 우클릭 메뉴를 연다.
    #[cfg(feature = "automation")]
    pub(crate) fn open_detached_menu(
        &mut self,
        id: u32,
        cx: &mut Context<Self>,
    ) {
        if let Some(detached) = self.detached_windows.get(&id) {
            detached.view.update(cx, |detached_root, detached_cx| detached_root.open_menu_for_automation(detached_cx));
        }
    }

    /// 자동 점검: 분리 창 `id`의 열린 메뉴에서 이름이 `label`인 항목을 고른 것과 같은 동작을 한다. 메뉴는 닫힌다.
    /// `window`는 본 창이다 — 묻기 창과 활성화는 본 창에서 일어난다.
    #[cfg(feature = "automation")]
    pub(crate) fn run_detached_menu_item(
        &mut self,
        id: u32,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(detached) = self.detached_windows.get(&id) {
            detached.view.update(cx, |detached_root, detached_cx| detached_root.close_menu(detached_cx));
        }
        let action = crate::work::menu_model::detached_menu_items(id)
            .into_iter()
            .find(|entry| entry.label == label)
            .map(|entry| entry.action);
        match action {
            Some(MenuAction::MoveBack(id)) => self.move_back(id, None, window, cx),
            Some(MenuAction::CloseTerminal(id)) => self.request_close_from_detached(id, window, cx),
            _ => {}
        }
    }

    /// 분리 창을 모두 닫는다. 본 창이 닫히기 전에 부른다 — 창이 하나라도 남으면 앱이 끝나지 않는다.
    pub(crate) fn close_detached_windows(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let ids: Vec<u32> = self.detached_windows.keys().copied().collect();
        for id in ids {
            self.close_detached_window(id, cx);
        }
        let keys: Vec<String> = self.detached_file_windows.keys().cloned().collect();
        for key in keys {
            self.close_detached_file_window(&key, cx);
        }
    }

    /// 메뉴가 터미널에 줄 창 옮기기 항목: 이 창에 있으면 `Move to New Window`, 다른 창에 있으면 `Move Back to Main Window`.
    /// 터미널 행·묶음 행·탭의 메뉴에만 있다(묶음 행은 마지막으로 쓴 터미널이 대상이다).
    pub(crate) fn window_move_action(
        &self,
        kind: &MenuKind,
    ) -> Option<MenuAction> {
        let id = match kind {
            MenuKind::Terminal(id) => Some(*id),
            MenuKind::Group(root) => {
                self.workspace.work().groups.iter().find(|group| group.root == *root).map(|group| group.last_used)
            }
            _ => None,
        };
        id.filter(|id| self.workspace.tab(*id).is_some()).map(|id| {
            if self.workspace.is_detached(id) { MenuAction::MoveBack(id) } else { MenuAction::MoveToNewWindow(id) }
        })
    }

    /// 다른 창에 있는 터미널 `id`의 `Show to the Side`: 이 창의 선택한 터미널 칸 오른쪽에 새 칸으로 되돌린다.
    pub(crate) fn move_back_beside(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reference = self.beside_reference_for_detached(id);
        let place = reference.map(|reference| DockPlace { reference, mode: DropMode::SplitRight });
        self.move_back(id, place, window, cx);
    }

    /// 다른 창의 터미널이 돌아올 때 기준이 될 이 창의 터미널: 지금 보는 배치에서 `id`가 아닌 선택한 터미널(없으면 첫 칸의 선택된 탭).
    pub(crate) fn beside_reference_for_detached(
        &self,
        id: u32,
    ) -> Option<u32> {
        self.workspace.view_layout().and_then(|layout| layout.beside_reference(id, self.workspace.selected()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_d81_moving_a_switching_tab_is_refused() {
        assert_eq!(move_refusal(true), Some("Wait for the account switch to finish before moving this tab."));
        assert_eq!(move_refusal(false), None);
    }
}
