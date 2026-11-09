//! 분리 창: 본 창에서 떼어 낸 탭 하나(터미널 또는 파일)를 따로 보이는 창.
//!
//! 제목줄, 탭 하나짜리 탭 줄(이름·읽지 않은 완료 점이나 저장하지 않은 점·×), 터미널 보기나 파일 내용만 있다. Work 목록·상태 줄·묻기 창은 없고,
//! 터미널 세션과 탭의 소속·이름·완료 알림, 파일 내용·편집기·저장하지 않은 표시는 본 창(`WorkWindow`)이 그대로 쥔다. 분리 창이 하는 일은
//! 본 창이 하는 일의 요청이다: 창을 닫으면 탭을 본 창으로 되돌리고, ×나 `Ctrl+Shift+W`는 본 창에 닫기 확인을 연다.
//! 파일 분리 창은 본 창이 만든 파일 내용 요소를 `render_detached_file`로 빌려 그리고, 저장·줄로 가기·검토 메모 키를 본 창에 넘긴다.
//!
//! 본 창의 메서드를 부를 때는 반드시 본 창의 창 손잡이로 부른다(`with_main`). 분리 창의 `window`로 묻기·메뉴·포커스를 열면
//! 그 요소가 분리 창에 열려 본 창이 그리지 못한다.
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::component::input::InputState;
use gpui_kit::{
    AnyWindowHandle, App, Context, DispatchPhase, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render,
    Styled, WeakEntity, Window, canvas, div, px,
};
use paddock_core::app_shortcut::file_shortcut;
use paddock_core::app_shortcut::{AppShortcut, app_shortcut};
use paddock_core::file_kind::file_name_of;
use paddock_core::key_encoding::KeyInput;
use paddock_core::tab_drop::{HINT_MOVE_BACK, HINT_MOVE_BACK_FILE};
use paddock_core::theme::{editor, file_tab, menu as menu_size, tab, tab_drag as drag_size, text};
use paddock_core::window_point::is_outside;

use crate::chrome::{ui_family, unseen_dot, zpx};
use crate::files::images::ImageMirror;
use crate::files::pane::{BodyHandlers, FileBodyModel, file_body_element};
use crate::look::{colors, scaled};
use crate::tab_drag::{drag_box, screen_bounds};
use crate::tab_drag_model::DetachedDrag;
use crate::terminal::find_bar::find_bar_view;
use crate::terminal::paint::hsla;
use crate::terminal::view::TerminalView;
use crate::titlebar::titlebar_view;
use crate::window_view::WorkWindow;
use crate::work::menu_model::{MenuAction, detached_menu_items};

/// 본 창이 분리 창의 겉모습(제목·크기·활성)을 읽는 공유 자리. 분리 창이 그릴 때와 활성이 바뀔 때 채운다.
pub struct DetachedShared {
    /// 창 제목(탭 이름).
    pub title: RefCell<String>,
    /// 마지막으로 그린 창 크기(논리 px).
    pub size: Cell<(f32, f32)>,
    /// 창이 활성인지.
    pub active: Cell<bool>,
    /// 본 창이 이 창을 활성으로 만들라고 요청한 횟수. 자동 점검이 화면 밖 창을 실제로 활성화하지 않고 요청만 세는 데 쓴다.
    pub n_activation_requests: Cell<u32>,
}

impl DetachedShared {
    pub fn new(title: &str) -> Self {
        Self {
            title: RefCell::new(title.to_string()),
            size: Cell::new((0.0, 0.0)),
            active: Cell::new(false),
            n_activation_requests: Cell::new(0),
        }
    }
}

/// 분리 창이 보여 주는 것.
pub enum DetachedBody {
    /// 터미널 하나: 번호와 그 터미널 보기(이 창으로 옮겨 그린다).
    Terminal { id: u32, view: Entity<TerminalView> },
    /// 파일 탭 하나: 탭 열쇠. 내용은 본 창이 쥐고 있다.
    File { key: String },
}

/// 분리 창 메뉴 항목이 하는 일.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuCommand {
    MoveBack,
    Close,
}

/// 분리 창의 뿌리 보기.
pub struct DetachedWindow {
    main: Entity<WorkWindow>,
    main_window: AnyWindowHandle,
    body: DetachedBody,
    focus: FocusHandle,
    shared: Rc<DetachedShared>,
    /// 읽지 않은 완료 알림이 있는지(본 창이 쥔 값의 사본, 터미널 창).
    unseen: bool,
    /// 파일에 저장하지 않은 변경이 있는지(본 창이 쥔 값의 사본, 파일 창).
    unsaved: bool,
    /// 열려 있는 우클릭 메뉴의 자리(창 안 좌표).
    menu: Option<Point<Pixels>>,
    /// 탭에서 시작한 끌기(본 창 칸·탭 줄로 되돌리는 끌기). 누른 자리에서 8px를 넘게 움직이면 끌기가 시작된다.
    drag: Option<DetachedDrag>,
    /// 끌기 중 마우스의 창 안 좌표(끌기 상자를 그릴 자리).
    drag_position: (f32, f32),
    /// 이 창 터미널에 붙은 찾기 띠의 입력칸과 `N of M` 글자(본 창이 쥔 찾기 상태의 사본). 본 창이 바뀔 때마다 다시 읽는다 — 그리는 동안에는
    /// 본 창을 빌리지 않는다.
    find: Option<(Entity<InputState>, String)>,
    /// 파일 창이 그리는 파일 내용의 사본(본 창이 쥔 내용의 핸들). 본 창이 바뀔 때마다 다시 만든다.
    file_model: Option<FileBodyModel>,
    /// 파일 창이 그린 이미지(PDF 쪽·이미지 보기)의 반납 대기 목록. 텍스처 보관소는 창마다 따로라 이 창이 직접 비운다.
    images: Option<ImageMirror>,
}

// 이 창이 그리는 동안에는 본 창(`WorkWindow`) 엔티티를 읽거나 갱신하지 않는다: gpui는 그리는 동안 건드린 엔티티의 "현재 창"을 그 창으로 바꾸고,
// 본 창의 비동기 이어받기(`update_in`)·`observe_in`·`subscribe_in`은 그 현재 창에서 돈다. 본 창 쪽 값(찾기 띠·파일 내용)은 본 창이 바뀔 때 받아 둔
// 사본(`find`·`file_model`)으로 그리고, 단추·키 처리기만 눌렸을 때 본 창에 일을 시킨다.

impl DetachedWindow {
    /// 분리 창을 만든다. 창을 닫으려 하면 탭을 본 창으로 되돌린다.
    // 창을 만드는 동안 본 창은 갱신 중이라 읽을 수 없어, 처음 포커스·내용 사본까지 호출자가 건네야 해 인자가 많다.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        main: Entity<WorkWindow>,
        main_window: AnyWindowHandle,
        body: DetachedBody,
        initial_focus: Option<FocusHandle>,
        file_model: Option<FileBodyModel>,
        images: Option<ImageMirror>,
        shared: Rc<DetachedShared>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        // 처음 키 입력을 받을 자리: 터미널 보기, 파일 창은 호출자가 건넨 편집기(없으면 창 자신). 창을 만드는 동안 본 창은 갱신 중이라 읽을 수 없다.
        let wanted = match &body {
            DetachedBody::Terminal { view, .. } => Some(view.focus_handle(cx)),
            DetachedBody::File { .. } => initial_focus,
        };
        window.focus(wanted.as_ref().unwrap_or(&focus), cx);
        let this = Self {
            main: main.clone(),
            main_window,
            body,
            focus,
            shared,
            unseen: false,
            unsaved: false,
            menu: None,
            drag: None,
            drag_position: (0.0, 0.0),
            find: None,
            file_model,
            images,
        };
        let entity = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_window, cx| {
            entity.update(cx, |this, cx| this.on_close_requested(cx)).unwrap_or(true)
        });
        // 본 창의 상태가 바뀔 때마다 이름·읽지 않은 점·저장하지 않은 점을 다시 읽고, 탭이 더는 다른 창에 있지 않으면(닫혔거나 되돌아갔다) 이 창을 닫는다.
        cx.observe_in(&main, window, move |this, main, window, cx| {
            let work = main.read(cx);
            let (is_here, title, unseen, unsaved) = match &this.body {
                DetachedBody::Terminal { id, .. } => (
                    work.workspace.is_detached(*id),
                    work.workspace.name(*id).to_string(),
                    work.done.is_unseen(*id),
                    false,
                ),
                DetachedBody::File { key } => match work.workspace.detached_file(key) {
                    Some(detached) => (true, file_name_of(&detached.tab.path), false, work.unsaved.contains(key)),
                    None => (false, String::new(), false, false),
                },
            };
            if is_here {
                this.find = match &this.body {
                    DetachedBody::Terminal { id, .. } => work
                        .find
                        .as_ref()
                        .filter(|bar| bar.terminal == *id)
                        .map(|bar| (bar.input.clone(), bar.state.count_text())),
                    DetachedBody::File { .. } => None,
                };
                if let DetachedBody::File { key } = &this.body {
                    this.file_model =
                        work.workspace.detached_file(key).map(|detached| FileBodyModel::of(work, Some(&detached.tab)));
                }
                if *this.shared.title.borrow() != title {
                    window.set_window_title(&title);
                    *this.shared.title.borrow_mut() = title;
                }
                this.unseen = unseen;
                this.unsaved = unsaved;
                cx.notify();
            } else {
                window.remove_window();
            }
        })
        .detach();
        let shared = this.shared.clone();
        cx.observe_window_activation(window, move |this, window, cx| {
            shared.active.set(window.is_window_active());
            // 창이 비활성이 되면 끝나지 않은 끌기를 버린다(창 밖 MouseUp을 받지 못한 경우의 잔존 방지).
            if !window.is_window_active() {
                this.end_drag(cx);
            }
        })
        .detach();
        this
    }

    /// 이 창이 다른 터미널 `terminal`을 이어서 보이게 한다(계정 전환으로 탭의 터미널이 바뀌었을 때). 창은 그대로다.
    pub fn retarget(
        &mut self,
        terminal: u32,
        view: Entity<TerminalView>,
        cx: &mut Context<Self>,
    ) {
        self.body = DetachedBody::Terminal { id: terminal, view };
        self.menu = None;
        cx.notify();
    }

    /// 이 창이 다른 파일 탭 `key`를 이어서 보이게 한다(파일 이름이 바뀌어 탭 열쇠가 바뀌었을 때). 창은 그대로다.
    pub fn retarget_file(
        &mut self,
        key: String,
        cx: &mut Context<Self>,
    ) {
        self.body = DetachedBody::File { key };
        self.menu = None;
        cx.notify();
    }

    /// 이 창이 보여 주는 터미널 번호(파일 창이면 None).
    fn terminal(&self) -> Option<u32> {
        match &self.body {
            DetachedBody::Terminal { id, .. } => Some(*id),
            DetachedBody::File { .. } => None,
        }
    }

    /// 키 입력을 받을 자리: 터미널 보기, 파일 창이면 파일 내용(편집기). 없으면 창 자신이다.
    fn content_focus(
        &self,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        let handle = match &self.body {
            DetachedBody::Terminal { view, .. } => Some(view.focus_handle(cx)),
            DetachedBody::File { key } => {
                self.main.read(cx).file_views.get(key).and_then(|content| content.focus_handle(cx))
            }
        };
        handle.unwrap_or_else(|| self.focus.clone())
    }

    /// 메뉴 항목: 이름, 위험 표시, 하는 일.
    fn menu_entries(&self) -> Vec<(String, bool, MenuCommand)> {
        match &self.body {
            DetachedBody::Terminal { id, .. } => detached_menu_items(*id)
                .into_iter()
                .map(|item| {
                    let command = if matches!(item.action, MenuAction::MoveBack(_)) {
                        MenuCommand::MoveBack
                    } else {
                        MenuCommand::Close
                    };
                    (item.label, item.danger, command)
                })
                .collect(),
            DetachedBody::File { .. } => vec![
                ("Move Back to Main Window".to_string(), false, MenuCommand::MoveBack),
                ("Close".to_string(), true, MenuCommand::Close),
            ],
        }
    }

    /// 자동 점검: 탭 우클릭 메뉴를 연다(사람이 탭을 오른쪽 눌렀을 때와 같은 자리).
    #[cfg(feature = "automation")]
    pub fn open_menu_for_automation(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.menu = Some(gpui_kit::point(gpui_kit::px(40.0), gpui_kit::px(80.0)));
        cx.notify();
    }

    /// 열려 있는 메뉴를 닫는다.
    #[cfg(feature = "automation")]
    pub fn close_menu(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        cx.notify();
    }

    /// 본 창에 일을 시킨다: 본 창의 손잡이로 본 창을 빌려 `work`를 실행한다. 본 창이 지금 갱신 중이면 하지 못하고 None이다.
    fn with_main<R>(
        &self,
        cx: &mut App,
        work: impl FnOnce(&mut WorkWindow, &mut Window, &mut Context<WorkWindow>) -> R,
    ) -> Option<R> {
        let main = self.main.clone();
        self.main_window
            .update(cx, |_, main_window, cx| {
                main.update(cx, |work_window, main_cx| work(work_window, main_window, main_cx))
            })
            .ok()
    }

    /// 창 닫기 요청: 탭을 본 창의 첫 칸 끝으로 돌려보내고 창을 닫는다.
    fn on_close_requested(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        self.move_back(cx);
        true
    }

    /// 본 창에 이 탭을 되돌려 달라고 한다(본 창이 이 창을 닫는다).
    fn move_back(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        match &self.body {
            DetachedBody::Terminal { id, .. } => {
                let terminal = *id;
                self.with_main(cx, |work, main_window, main_cx| work.move_back(terminal, None, main_window, main_cx));
            }
            DetachedBody::File { key } => {
                let key = key.clone();
                self.with_main(cx, |work, main_window, main_cx| work.move_file_back(&key, main_window, main_cx));
            }
        }
    }

    /// 본 창에 이 터미널의 닫기를 요청한다(셸이 살아 있으면 본 창에 확인 창이 열리고 본 창이 앞으로 온다).
    fn request_close(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        match &self.body {
            DetachedBody::Terminal { id, .. } => {
                let terminal = *id;
                self.with_main(cx, |work, main_window, main_cx| {
                    work.request_close_from_detached(terminal, main_window, main_cx)
                });
            }
            DetachedBody::File { key } => {
                let key = key.clone();
                self.with_main(cx, |work, main_window, main_cx| {
                    work.request_close_detached_file(&key, main_window, main_cx)
                });
            }
        }
        cx.notify();
    }

    /// 이 창의 터미널 `terminal`에 찾기 띠를 연다. 찾기 상태는 본 창이 쥐고(하나뿐이다) 입력칸·띠는 이 창에 그려진다.
    fn open_find(
        &mut self,
        terminal: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.main.update(cx, |work, main_cx| work.open_terminal_find_for(terminal, window, main_cx));
        cx.notify();
    }

    /// 끌기 상태를 버리고 본 창에 그려 둔 놓을 자리 미리보기를 지운다.
    fn end_drag(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.drag.take().is_some() {
            // 터미널 창은 본 창에 그려 둔 놓을 자리 미리보기를 지운다(파일 창은 미리보기가 없다).
            if let Some(terminal) = self.terminal() {
                self.with_main(cx, |work, main_window, main_cx| {
                    work.set_dock_preview_from_screen(terminal, None, main_window, main_cx)
                });
            }
            cx.notify();
        }
    }

    /// 마우스 `position`(이 창 안 좌표, 창 밖이면 음수나 창 크기를 넘는 값)의 화면 좌표.
    fn screen_point(
        window: &Window,
        position: (f32, f32),
    ) -> (f32, f32) {
        let bounds = screen_bounds(window);
        (bounds.left + position.0, bounds.top + position.1)
    }

    /// 누른 채 움직였다: 8px를 넘으면 끌기가 시작되고, 마우스가 이 창 밖이면 본 창에 그 점의 놓을 자리를 미리보기로 보이게 한다.
    /// 마우스 버튼이 이미 떼어져 있으면(뗌을 못 받았다) 끌기를 버린다.
    fn drag_moved(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.end_drag(cx);
        } else if let Some(drag) = self.drag.as_mut() {
            let position = (f32::from(event.position.x), f32::from(event.position.y));
            if drag.moved(position) {
                self.drag_position = position;
                if let Some(terminal) = self.terminal() {
                    let viewport = window.viewport_size();
                    let outside =
                        is_outside(position.0, position.1, f32::from(viewport.width), f32::from(viewport.height));
                    let screen = outside.then(|| Self::screen_point(window, position));
                    self.with_main(cx, |work, main_window, main_cx| {
                        work.set_dock_preview_from_screen(terminal, screen, main_window, main_cx)
                    });
                }
                cx.notify();
            }
        }
    }

    /// 왼쪽 버튼을 뗐다. 끌던 중이고 이 창 밖에서 뗐으면 그 점이 본 창의 칸이나 탭 줄 위일 때 탭을 거기로 되돌린다.
    /// 이 창 안에서 뗐거나 본 창 밖이면 아무 일도 없다.
    fn drag_released(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let drag = self.drag.take();
        if drag.is_some_and(|drag| drag.is_active()) {
            let position = (f32::from(event.position.x), f32::from(event.position.y));
            let viewport = window.viewport_size();
            let outside = is_outside(position.0, position.1, f32::from(viewport.width), f32::from(viewport.height));
            let screen = Self::screen_point(window, position);
            match &self.body {
                DetachedBody::Terminal { id, .. } => {
                    let terminal = *id;
                    let docked = outside
                        && self
                            .with_main(cx, |work, main_window, main_cx| {
                                work.dock_from_detached(terminal, screen, main_window, main_cx)
                            })
                            .unwrap_or(false);
                    if !docked {
                        self.with_main(cx, |work, main_window, main_cx| {
                            work.set_dock_preview_from_screen(terminal, None, main_window, main_cx)
                        });
                    }
                }
                DetachedBody::File { key } => {
                    // 파일 탭은 본 창 어디에 놓아도 그 묶음 파일 칸으로 돌아온다(칸 자리를 고르지 않는다).
                    let key = key.clone();
                    if outside {
                        self.with_main(cx, |work, main_window, main_cx| {
                            work.dock_file_from_detached(&key, screen, main_window, main_cx)
                        });
                    }
                }
            }
        }
        cx.notify();
    }

    fn run_menu_command(
        &mut self,
        command: MenuCommand,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        match command {
            MenuCommand::MoveBack => self.move_back(cx),
            MenuCommand::Close => self.request_close(cx),
        }
        cx.notify();
    }

    /// 창 전체가 받는 키: 메뉴가 열려 있으면 Esc로 닫고, `Ctrl+Shift+W`는 이 터미널 닫기를 요청한다. 나머지는 터미널이 받는다.
    fn on_capture_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        let key_name = crate::platform_keys::shortcut_key(&event.keystroke.key);
        if key_name == "escape" && self.drag.is_some() {
            // 탭을 끌던 중의 Esc: 끌기를 취소한다(뗄 때 아무 일도 없다).
            self.end_drag(cx);
            cx.stop_propagation();
        } else if key_name == "escape" && self.menu.is_none() && self.main.read(cx).find_input_focused(window, cx) {
            // 찾기 입력칸에서 Esc: 띠를 닫고 키 입력을 이 창의 터미널로 돌린다.
            self.main.update(cx, |work, main_cx| work.close_terminal_find(window, main_cx));
            cx.stop_propagation();
        } else if self.menu.is_some() {
            if key_name == "escape" {
                self.menu = None;
                window.focus(&self.content_focus(cx), cx);
                cx.notify();
            }
            cx.stop_propagation();
        } else if let DetachedBody::File { key } = &self.body
            && !modifiers.platform
            && let Some(shortcut) = file_shortcut(&KeyInput {
                key: &key_name,
                key_char: event.keystroke.key_char.as_deref(),
                control: modifiers.control,
                shift: modifiers.shift,
                alt: modifiers.alt,
            })
        {
            // 저장·줄로 가기·검토 메모: 본 창이 이 파일을 "지금 파일"로 두고 실행한다(묻기 창은 본 창에 열린다).
            let key = key.clone();
            let handled = self
                .with_main(cx, |work, main_window, main_cx| {
                    work.run_detached_file_shortcut(&key, shortcut, main_window, main_cx)
                })
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        } else if !modifiers.platform
            && let Some(shortcut) = app_shortcut(&KeyInput {
                key: &key_name,
                key_char: event.keystroke.key_char.as_deref(),
                control: modifiers.control,
                shift: modifiers.shift,
                alt: modifiers.alt,
            })
        {
            match shortcut {
                // 이 창의 터미널을 닫는다(확인 창은 본 창에 열린다).
                AppShortcut::CloseActiveTab => self.request_close(cx),
                // 터미널 찾기 띠: 본 창이 쥔 찾기 상태를 이 창 터미널에 붙여 이 창에 그린다. 파일 창에는 터미널이 없어 키를 거두기만 한다.
                AppShortcut::FindInTerminal => {
                    if let Some(terminal) = self.terminal() {
                        self.open_find(terminal, window, cx);
                    }
                }
                // 나머지 앱 단축키(새 터미널·분할·사이드바·탭 이동·검색·빠른 열기)는 본 창을 앞으로 가져와 본 창에서 실행한다.
                other => {
                    self.with_main(cx, |work, main_window, main_cx| {
                        work.activate_main(main_window);
                        work.run_shortcut(other, main_window, main_cx)
                    });
                }
            }
            cx.stop_propagation();
        }
    }

    /// 탭 하나짜리 탭 줄. 파일 창은 파일 그림이 앞에 있고, 저장하지 않은 변경이 있으면 × 자리에 점(`●`)이 보이다가 마우스를 올리면 ×가 된다.
    fn tab_strip(
        &self,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let palette = colors();
        let title = self.shared.title.borrow().clone();
        let is_file = matches!(self.body, DetachedBody::File { .. });
        let show_dot = is_file && self.unsaved;
        let glyph = if show_dot {
            div()
                .flex()
                .items_center()
                .justify_center()
                .child(div().group_hover("detached-tab-close", |style| style.hidden()).child(editor::UNSAVED_DOT))
                .child(div().child("\u{2715}").hidden().group_hover("detached-tab-close", |style| style.block()))
        } else {
            div().child("\u{2715}")
        };
        let close = div()
            .group("detached-tab-close")
            .flex_none()
            .w(zpx(tab::CLOSE_SIZE))
            .h(zpx(tab::CLOSE_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .rounded(zpx(tab::CLOSE_RADIUS))
            .text_size(zpx(tab::CLOSE_GLYPH))
            .text_color(hsla(if show_dot { palette.fg_strong } else { palette.fg_muted }))
            .cursor_pointer()
            .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
            .child(glyph)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    this.request_close(cx);
                }),
            );
        let file_icon = is_file.then(|| {
            div()
                .flex_none()
                .w(zpx(file_tab::ICON_WIDTH))
                .h(zpx(file_tab::ICON_HEIGHT))
                .mr(zpx(file_tab::ICON_GAP))
                .border_1()
                .border_color(hsla(palette.fg_muted))
                .rounded(zpx(file_tab::ICON_RADIUS))
        });
        let tab_item = div()
            .id("detached-tab")
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w(zpx(tab::WIDTH))
            .h(zpx(tab::HEIGHT))
            .pl(zpx(if is_file { file_tab::PADDING_LEFT } else { tab::TEXT_PADDING_LEFT }))
            .pr(zpx(tab::PADDING_RIGHT))
            .rounded_t(zpx(tab::RADIUS_TOP))
            .bg(hsla(palette.tab_active_bg))
            .text_color(hsla(palette.tab_active_fg))
            .cursor_pointer()
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .children(file_icon)
                    .child(div().min_w_0().truncate().child(title))
                    .children(self.unseen.then(unseen_dot)),
            )
            .child(close)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.content_focus(cx), cx);
                    // 탭을 누른 채 끌면 본 창으로 되돌리는 끌기가 된다.
                    this.drag = Some(DetachedDrag::new((f32::from(event.position.x), f32::from(event.position.y))));
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    this.menu = Some(event.position);
                    cx.notify();
                }),
            );
        div()
            .flex()
            .flex_row()
            .items_end()
            .flex_none()
            .h(zpx(tab::BAR_HEIGHT))
            .w_full()
            .pl(zpx(tab::BAR_PADDING_LEFT))
            .border_b_1()
            .border_color(hsla(palette.border))
            .bg(hsla(palette.tab_bar_bg))
            .text_size(zpx(text::SMALL))
            .child(tab_item)
    }

    /// 우클릭 메뉴: 창 전체를 덮는 투명 막과 항목 판. 항목은 `Move Back to Main Window`, `Close terminal`이다.
    fn menu_panel(
        &self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let palette = colors();
        let mut panel = div()
            .id("detached-menu")
            .absolute()
            .left(position.x)
            .top(position.y)
            .min_w(zpx(menu_size::MIN_WIDTH))
            .p(zpx(menu_size::PADDING))
            .flex()
            .flex_col()
            .rounded(zpx(menu_size::RADIUS))
            .border_1()
            .border_color(hsla(palette.popup_border))
            .bg(hsla(palette.popup_bg))
            .text_color(hsla(palette.fg))
            .text_size(zpx(menu_size::FONT_SIZE))
            .shadow_lg()
            .occlude();
        for (index, (label, danger, command)) in self.menu_entries().into_iter().enumerate() {
            let color = if danger { palette.danger } else { palette.fg_strong };
            panel = panel.child(
                div()
                    .id(("detached-menu-item", index))
                    .flex()
                    .flex_row()
                    .items_center()
                    .px(zpx(menu_size::ITEM_PADDING_X))
                    .py(zpx(menu_size::ITEM_PADDING_Y))
                    .rounded(zpx(menu_size::ITEM_RADIUS))
                    .text_color(hsla(color))
                    .cursor_pointer()
                    .hover(|style| style.bg(hsla(colors().selected_bg)))
                    .child(div().flex_1().min_w_0().truncate().child(label))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.run_menu_command(command, cx);
                            window.focus(&this.content_focus(cx), cx);
                        }),
                    ),
            );
        }
        div()
            .id("detached-menu-scrim")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseDownEvent, _window, cx| {
                    this.menu = None;
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _event: &MouseDownEvent, _window, cx| {
                    this.menu = None;
                    cx.notify();
                }),
            )
            .child(panel)
    }
}

impl Focusable for DetachedWindow {
    fn focus_handle(
        &self,
        _cx: &App,
    ) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DetachedWindow {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = colors();
        let viewport = window.viewport_size();
        self.shared.size.set((f32::from(viewport.width), f32::from(viewport.height)));
        self.shared.active.set(window.is_window_active());
        // 이 창에서 그린 이미지의 그래픽 메모리를 돌려준다(본 창의 반납은 본 창의 텍스처 보관소만 비운다). 본 창과 같이 한 프레임 뒤 한 번 더 그린다.
        let buried = self.images.as_ref().map(ImageMirror::drain).unwrap_or_default();
        let has_buried = !buried.is_empty();
        for image in buried {
            let _ = window.drop_image(image);
        }
        if has_buried {
            // 본 창도 같은 이미지를 자기 보관소에서 비우고 놓아 주도록 같이 다시 그리게 한다.
            let main_window = self.main_window;
            window.on_next_frame(move |window, cx| {
                window.refresh();
                let _ = main_window.update(cx, |_, main, _| main.refresh());
            });
        }
        let title = self.shared.title.borrow().clone();
        let root = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(hsla(palette.app_bg))
            .text_color(hsla(palette.fg))
            .font_family(ui_family())
            .text_size(zpx(text::UI))
            .track_focus(&self.focus)
            .key_context("DetachedWindow")
            .capture_key_down(cx.listener(Self::on_capture_key))
            .children(titlebar_view(window, &title))
            .child(self.tab_strip(cx));
        let root = match &self.body {
            DetachedBody::Terminal { view, .. } => {
                // 터미널 찾기 띠(본 창이 쥔 찾기 상태가 이 터미널을 가리킬 때): 입력칸과 단추는 이 창에 그려진다.
                let find_bar = self.find.clone().map(|(input, count)| {
                    find_bar_view(
                        &input,
                        &count,
                        cx,
                        move |this: &mut Self, forward, _window, cx| {
                            this.main.update(cx, |work, main_cx| work.find_step(forward, main_cx))
                        },
                        |this: &mut Self, window, cx| {
                            this.main.update(cx, |work, main_cx| work.close_terminal_find(window, main_cx))
                        },
                    )
                });
                root.child(div().flex_1().min_h_0().relative().child(view.clone()).children(find_bar))
            }
            DetachedBody::File { .. } => {
                let body = self.file_model.clone().map(|model| {
                    div()
                        .id("detached-file")
                        .size_full()
                        .bg(hsla(palette.terminal_bg))
                        .overflow_hidden()
                        .child(div().size_full().child(file_body_element(&model, cx, &file_handlers())))
                });
                root.child(div().flex_1().min_h_0().relative().children(body))
            }
        };
        let root = root.child(drag_tracker(cx.entity().downgrade()));
        #[cfg(feature = "automation")]
        let root = root.child(mouse_probe(self.terminal().unwrap_or(0)));
        // 끌기 중에는 이름과 안내를 적은 상자가 마우스를 따라다닌다(마우스가 이 창 밖이면 창 밖이라 보이지 않는다).
        let hint = if matches!(self.body, DetachedBody::File { .. }) { HINT_MOVE_BACK_FILE } else { HINT_MOVE_BACK };
        let dragged_box = self.drag.filter(DetachedDrag::is_active).map(|_| {
            div()
                .absolute()
                .left(px(self.drag_position.0 + scaled(drag_size::CURSOR_OFFSET)))
                .top(px(self.drag_position.1 + scaled(drag_size::CURSOR_OFFSET)))
                .child(drag_box(&title, hint))
        });
        root.children(dragged_box).children(self.menu.map(|position| self.menu_panel(position, cx)))
    }
}

/// 파일 분리 창의 파일 내용 위 단추(안내 탭 단추·디스크 변경 띠)가 부를 동작: 본 창에 일을 시키고, 키 입력은 이 창의 파일 내용에 돌려준다.
fn file_handlers() -> BodyHandlers<DetachedWindow> {
    BodyHandlers {
        // 본 창 메서드에는 본 창의 손잡이(`with_main`)를 넘긴다: 이 창의 `window`를 넘기면 읽기가 끝난 뒤 본 창 편집기 포커스가 이 창에 잡힌다.
        on_notice: Rc::new(|this, action, path, window, cx| {
            this.with_main(cx, |work, main_window, main_cx| work.run_notice_action(action, path, main_window, main_cx));
            window.focus(&this.content_focus(cx), cx);
        }),
        on_reload: Rc::new(|this, path, window, cx| {
            this.with_main(cx, |work, main_window, main_cx| work.reload_from_disk(path, main_window, main_cx));
            window.focus(&this.content_focus(cx), cx);
        }),
        on_keep: Rc::new(|this, path, window, cx| {
            this.main.update(cx, |work, main_cx| work.keep_mine(path, main_cx));
            window.focus(&this.content_focus(cx), cx);
        }),
    }
}

/// 이 창이 받는 마우스 이동·뗌을 창 밖 좌표까지 끌기에 전한다. 마우스를 누른 창은 창 밖에서도 이벤트를 받지만 요소 단위 처리기는
/// 창 안에서만 불리므로, 창 단위로 직접 받는다(캡처 단계라 다른 요소가 거두지 못한다).
fn drag_tracker(window_view: WeakEntity<DetachedWindow>) -> impl IntoElement {
    canvas(
        |bounds, _window, _cx| bounds,
        move |_bounds, _prepaint, window, _cx| {
            let moves = window_view.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    let _ = moves.update(cx, |this, cx| this.drag_moved(event, window, cx));
                }
            });
            let ups = window_view.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                    let _ = ups.update(cx, |this, cx| this.drag_released(event, window, cx));
                }
            });
        },
    )
    .absolute()
    .size_full()
}

/// 자동 점검: 이 창이 받는 마우스 누름·이동·뗌의 좌표를 `events.log`에 남긴다. 창 밖에서 뗀 점이 음수나 창 크기를 넘는 좌표로 오는지 읽는 데 쓴다.
#[cfg(feature = "automation")]
fn mouse_probe(terminal: u32) -> impl IntoElement {
    use gpui_kit::{DispatchPhase, MouseMoveEvent, MouseUpEvent, canvas};

    canvas(
        |bounds, _window, _cx| bounds,
        move |_bounds, _prepaint, window, _cx| {
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _cx| {
                if phase == DispatchPhase::Bubble {
                    let size = window.viewport_size();
                    crate::automation::event(&format!(
                        "detached-mouse id={terminal} down x={:.0} y={:.0} window={:.0}x{:.0}",
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                        f32::from(size.width),
                        f32::from(size.height)
                    ));
                }
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, _cx| {
                if phase == DispatchPhase::Bubble {
                    let size = window.viewport_size();
                    crate::automation::event(&format!(
                        "detached-mouse id={terminal} up x={:.0} y={:.0} window={:.0}x{:.0}",
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                        f32::from(size.width),
                        f32::from(size.height)
                    ));
                }
            });
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _cx| {
                if phase == DispatchPhase::Bubble && event.pressed_button.is_some() {
                    let size = window.viewport_size();
                    crate::automation::event(&format!(
                        "detached-mouse id={terminal} move x={:.0} y={:.0} window={:.0}x{:.0}",
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                        f32::from(size.width),
                        f32::from(size.height)
                    ));
                }
            });
        },
    )
    .absolute()
    .size_full()
}
