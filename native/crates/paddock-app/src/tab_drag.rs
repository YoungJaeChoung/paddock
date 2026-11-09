//! 탭 끌기(본 창): 탭을 칸 가장자리·가운데·탭 줄에 놓아 칸을 나누거나 옮기고, 창 밖에 놓아 분리 창으로 뗀다.
//!
//! 끌기 자체는 gpui의 `on_drag`/`on_drag_move`/`on_drop`이다. 놓을 자리는 마우스가 움직일 때마다 창 루트가 한곳에서 정한다
//! (`tab_drag_model::preview_at` — 마지막으로 그린 칸 본문·탭 줄 자리에 core의 `tab_drop` 규칙을 먹인다). 탭 위에서만 탭 자신이
//! 정확한 끼움 자리로 덮는다(`tab_drag_over_tab`). 놓을 때는 그 자리가 곧 놓을 곳이라 루트의 `on_drop` 하나가 맡는다.
//! 창 밖에서 뗀 것은 마우스를 누른 창이 창 밖 좌표로 받는다(`on_mouse_up_out`). 다른 창(분리 창)에서 끌어 온 탭은 분리 창이
//! 화면 좌표로 바꿔 이 창의 `dock_from_detached`를 부른다.
//!
//! `Esc`는 끌기를 취소로 표시하고 gpui의 끌기도 멈춘다(미리보기가 사라진다). 창이 비활성이 되거나 새로 마우스를 누르면 끌기 상태를 버린다.
use gpui_kit::{
    Context, Div, DragMoveEvent, IntoElement, ParentElement, Pixels, Point, Render, Styled, Window, div, point, px,
};
use paddock_core::tab_drop::{HINT_DETACH, HINT_DETACH_FILE, RowTab};
use paddock_core::theme::{dock_preview, tab, tab_drag};
use paddock_core::window_point::ScreenBounds;

use crate::chrome::zpx;
use crate::look::{colors, scaled};
use crate::tab_drag_model::{
    DockPreview, FileDrag, FileRelease, PaneSlot, Release, TabDrag, detached_origin, file_release_action,
    main_point_preview, preview_at, release_action, row_preview, terminal_of,
};
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;
use crate::workspace::{DetachedTab, DockPlace};

/// 끌고 있는 탭. 끄는 동안 마우스 곁에 `<이름> · <안내>` 상자로 보인다.
#[derive(Clone, Debug)]
pub struct DragTab {
    pub tab: DetachedTab,
    /// 탭에 보이는 이름.
    pub label: String,
    /// 이름 뒤에 붙는 안내 글.
    pub hint: &'static str,
}

impl DragTab {
    /// 이 창의 터미널 탭을 끄는 상태.
    pub fn terminal(
        id: u32,
        label: String,
    ) -> Self {
        Self { tab: DetachedTab::Terminal(id), label, hint: HINT_DETACH }
    }
}

/// 끌고 있는 파일 탭(지금 보는 묶음 파일 칸의 `index`번째). 칸에 놓을 수 없어 창 밖에 놓을 때만 새 창으로 뗀다.
#[derive(Clone, Debug)]
pub struct DragFileTab {
    pub index: usize,
    /// 탭에 보이는 이름.
    pub label: String,
}

impl Render for DragFileTab {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        drag_box(&self.label, HINT_DETACH_FILE)
    }
}

impl Render for DragTab {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        drag_box(&self.label, self.hint)
    }
}

/// 끌기 미리보기 상자: 안쪽 8/12, 1px 테두리, 모서리 6, `<이름> · <안내>`(안내는 흐린 글자).
pub fn drag_box(
    label: &str,
    hint: &str,
) -> Div {
    let palette = colors();
    div()
        .py(zpx(tab_drag::PADDING_Y))
        .px(zpx(tab_drag::PADDING_X))
        .flex()
        .flex_row()
        .items_center()
        .rounded(zpx(tab_drag::RADIUS))
        .border_1()
        .border_color(hsla(palette.popup_border))
        .bg(hsla(palette.popup_bg))
        .text_size(zpx(tab_drag::TEXT_SIZE))
        .text_color(hsla(palette.fg))
        .child(label.to_string())
        .child(div().text_color(hsla(palette.fg_muted)).child(format!(" \u{00b7} {hint}")))
}

/// 놓을 자리를 알리는 그림: 칸 위의 상자(2px `accent` 테두리 + `accent` 18 % 채움)나 탭 줄에 끼울 자리의 세로선(2px `accent`).
pub fn dock_preview_view(preview: &DockPreview) -> Div {
    let accent = hsla(colors().accent);
    let rect = preview.rect;
    let base = div().absolute().left(px(rect.left)).top(px(rect.top)).w(px(rect.width)).h(px(rect.height));
    if preview.line {
        base.bg(accent)
    } else {
        base.border(px(dock_preview::BORDER_WIDTH)).border_color(accent).bg(accent.opacity(dock_preview::FILL_ALPHA))
    }
}

fn tuple(point: Point<Pixels>) -> (f32, f32) {
    (f32::from(point.x), f32::from(point.y))
}

/// 창 `window`가 화면에서 차지한 본문 사각형(그리는 영역).
pub fn screen_bounds(window: &Window) -> ScreenBounds {
    let bounds = window.bounds();
    ScreenBounds {
        left: f32::from(bounds.origin.x),
        top: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
    }
}

impl WorkWindow {
    /// 지금 보는 묶음의 칸·탭 줄이 창 안에서 차지한 자리: 마지막으로 그린 본문 크기·위치와 지금 칸 배치로 만든다.
    pub(crate) fn dock_slots(&self) -> Vec<PaneSlot> {
        let origin = self.body_origin.get();
        self.workspace
            .view_layout()
            .map(|layout| {
                crate::tab_drag_model::pane_slots(&layout, self.body_size.get(), origin, scaled(tab::BAR_HEIGHT))
            })
            .unwrap_or_default()
    }

    /// 칸 배치에서 터미널 `id`가 든 칸의 탭(칸 안 순서). 이 창의 칸에 없으면 빈 목록이다.
    pub(crate) fn pane_tabs_of(
        &self,
        id: u32,
    ) -> Vec<u32> {
        self.workspace
            .view_layout()
            .and_then(|layout| layout.pane_boxes(1.0, 1.0).into_iter().find(|pane| pane.tabs.contains(&id)))
            .map(|pane| pane.tabs)
            .unwrap_or_default()
    }

    /// 창 루트가 끌기 중 마우스가 움직일 때마다 받는다: 끌기 상태를 갱신하고 마우스 아래의 놓을 자리를 미리보기로 정한다.
    /// 칸이나 탭 줄 밖이거나 창 밖이면 미리보기를 지운다. 탭 위에서는 이 뒤에 탭이 더 정확한 끼움 자리로 덮는다.
    pub(crate) fn tab_drag_moved(
        &mut self,
        event: &DragMoveEvent<DragTab>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let dragged = event.drag(cx).tab;
        let position = tuple(event.event.position);
        let mut state = match self.tab_drag.take() {
            Some(state) if state.tab == dragged => state,
            _ => TabDrag::new(dragged, position),
        };
        state.position = position;
        let viewport = window.viewport_size();
        let inside = !paddock_core::window_point::is_outside(
            position.0,
            position.1,
            f32::from(viewport.width),
            f32::from(viewport.height),
        );
        let next = (inside && !state.cancelled)
            .then(|| preview_at(&self.dock_slots(), terminal_of(dragged), position.0, position.1))
            .flatten();
        self.tab_drag = Some(state);
        if next != self.dock_preview {
            self.dock_preview = next;
        }
        cx.notify();
    }

    /// 창 루트가 파일 탭 끌기 중 마우스가 움직일 때마다 받는다: 끌기 상태만 갱신한다(파일 탭에는 놓을 자리 미리보기가 없다).
    pub(crate) fn file_drag_moved(
        &mut self,
        event: &DragMoveEvent<DragFileTab>,
        cx: &mut Context<Self>,
    ) {
        let index = event.drag(cx).index;
        let position = tuple(event.event.position);
        let mut state = match self.file_drag.take() {
            Some(state) if state.index == index => state,
            _ => FileDrag::new(index, position),
        };
        state.position = position;
        self.file_drag = Some(state);
    }

    /// 창 안에서 파일 탭을 놓았다: 파일 탭은 칸에 놓을 수 없어 아무 일도 없다.
    pub(crate) fn file_dropped(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.file_drag = None;
        cx.notify();
    }

    /// 탭 줄의 탭 `tab_id` 위에서 끌기 중 마우스가 움직였다: 마우스가 그 탭 안이면 왼쪽 절반은 앞, 오른쪽 절반은 뒤에 끼우는 자리로 미리보기를 덮는다.
    pub(crate) fn tab_drag_over_tab(
        &mut self,
        tab_id: u32,
        event: &DragMoveEvent<DragTab>,
        cx: &mut Context<Self>,
    ) {
        let bounds = event.bounds;
        let cancelled = self.tab_drag.is_some_and(|state| state.cancelled);
        if !cancelled && bounds.contains(&event.event.position) {
            let dragged = terminal_of(event.drag(cx).tab);
            let row = RowTab { id: tab_id, left: f32::from(bounds.left()), right: f32::from(bounds.right()) };
            self.dock_preview = row_preview(
                &self.pane_tabs_of(tab_id),
                dragged,
                row,
                f32::from(bounds.top()),
                f32::from(bounds.size.height),
                f32::from(event.event.position.x),
            );
            cx.notify();
        }
    }

    /// 창 안에서 놓았다: 미리보기가 가리키던 자리에 놓는다. 자리가 없으면 아무 일도 없다.
    pub(crate) fn tab_dropped(
        &mut self,
        dropped: &DragTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = self.tab_drag.take().unwrap_or_else(|| TabDrag::new(dropped.tab, (0.0, 0.0)));
        let preview = self.dock_preview.take();
        let viewport = window.viewport_size();
        let release = release_action(
            Some(&state),
            state.position,
            (f32::from(viewport.width), f32::from(viewport.height)),
            preview.as_ref(),
        );
        if let Release::Dock(place) = release {
            self.dock_tab(terminal_of(dropped.tab), place, window, cx);
        }
        cx.notify();
    }

    /// 마우스를 누른 창 밖에서 뗐다(창 밖 좌표로 온 MouseUp): 끌기 중이던 탭을 그 지점에 새 분리 창으로 뗀다.
    pub(crate) fn tab_released_outside(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_drag.is_some() {
            self.finish_file_release(tuple(position), window, cx);
        } else if self.tab_drag.is_some() {
            // 야간 점검(I44)이 실제 마우스로 창 밖에 놓은 지점이 창 밖 좌표로 오는지 읽는 기록이다.
            #[cfg(feature = "automation")]
            {
                let viewport = window.viewport_size();
                crate::automation::event(&format!(
                    "tab-drag outside-release x={:.0} y={:.0} window={:.0}x{:.0}",
                    f32::from(position.x),
                    f32::from(position.y),
                    f32::from(viewport.width),
                    f32::from(viewport.height)
                ));
            }
            self.finish_release(tuple(position), window, cx);
        }
    }

    /// 끌기를 `position`(창 안 좌표)에서 놓았다고 처리한다: 상태를 거두고, 창 밖이면 새 분리 창으로 떼고, 창 안이면(키를 누른 뒤 마우스를
    /// 움직이지 않고 놓으면 gpui가 창 안 점에도 `on_mouse_up_out`을 부른다) 마지막 미리보기 자리에 놓는다.
    pub(crate) fn finish_release(
        &mut self,
        position: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = self.tab_drag.take();
        let preview = self.dock_preview.take();
        let viewport = window.viewport_size();
        let release = release_action(
            state.as_ref(),
            position,
            (f32::from(viewport.width), f32::from(viewport.height)),
            preview.as_ref(),
        );
        if let (Release::Dock(place), Some(state)) = (release, state) {
            self.dock_tab(terminal_of(state.tab), place, window, cx);
        } else if let (Release::OpenWindow, Some(state)) = (release, state) {
            let origin = window.bounds().origin;
            let (left, top) = detached_origin((f32::from(origin.x) + position.0, f32::from(origin.y) + position.1));
            self.move_to_new_window(terminal_of(state.tab), Some(point(px(left), px(top))), window, cx);
            // 창이 열리면 이 창에는 MouseUp이 더 오지 않을 수 있다. gpui의 끌기도 여기서 끝낸다.
            cx.stop_active_drag(window);
        }
        cx.notify();
    }

    /// 파일 탭 끌기를 `position`(창 안 좌표)에서 놓았다고 처리한다: 창 밖이면 그 파일 탭을 지금 보는 묶음 칸에서 떼어 새 분리 창에 둔다.
    /// 창 안이면(칸·탭 줄 포함) 아무 일도 없다.
    pub(crate) fn finish_file_release(
        &mut self,
        position: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = self.file_drag.take();
        let viewport = window.viewport_size();
        let release =
            file_release_action(state.as_ref(), position, (f32::from(viewport.width), f32::from(viewport.height)));
        if let (FileRelease::OpenWindow, Some(state), Some(group)) = (release, state, self.workspace.view_key()) {
            let origin = window.bounds().origin;
            let (left, top) = detached_origin((f32::from(origin.x) + position.0, f32::from(origin.y) + position.1));
            self.move_file_to_new_window(group, state.index, Some(point(px(left), px(top))), window, cx);
            // 창이 열리면 이 창에는 MouseUp이 더 오지 않을 수 있다. gpui의 끌기도 여기서 끝낸다.
            cx.stop_active_drag(window);
        }
        cx.notify();
    }

    /// `Esc`: 끌기를 취소로 표시하고 gpui의 끌기도 멈춘다. 이 뒤의 놓기는 아무 일도 하지 않는다.
    pub(crate) fn cancel_tab_drag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.tab_drag.as_mut() {
            state.cancel();
        }
        if let Some(state) = self.file_drag.as_mut() {
            state.cancelled = true;
        }
        self.dock_preview = None;
        cx.stop_active_drag(window);
        cx.notify();
    }

    /// 창이 비활성이 되었거나 새로 마우스를 눌렀다: 끝나지 않은 끌기 상태(창 밖에서 뗀 MouseUp을 못 받은 경우)를 버린다.
    pub(crate) fn reset_tab_drag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab_drag.is_some() || self.dock_preview.is_some() || self.file_drag.is_some() {
            self.tab_drag = None;
            self.file_drag = None;
            self.dock_preview = None;
            cx.stop_active_drag(window);
            cx.notify();
        }
    }

    /// 터미널 `id`를 `place`로 옮긴다: 이 창의 탭이면 칸 배치를 바꾸고, 다른 창에 있던 탭이면 이 창으로 되돌린다. 옮긴 탭이 선택된다.
    pub(crate) fn dock_tab(
        &mut self,
        id: u32,
        place: DockPlace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.is_detached(id) {
            self.move_back(id, Some(place), window, cx);
        } else if self.workspace.dock_terminal(id, place) {
            self.select_tab(id, window, cx);
        }
    }

    /// 분리 창에서 끌어 온 터미널 `id`를 화면 좌표 `screen`에 놓았다. 그 점이 이 창의 칸이나 탭 줄 위면 되돌려 놓고 true다.
    pub(crate) fn dock_from_detached(
        &mut self,
        id: u32,
        screen: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let preview = main_point_preview(screen, screen_bounds(window), &self.dock_slots(), id);
        self.dock_preview = None;
        if let Some(preview) = preview {
            self.move_back(id, Some(preview.place), window, cx);
        }
        cx.notify();
        preview.is_some()
    }

    /// 분리 창에서 끌던 터미널 `id`가 화면 좌표 `screen`에 있다(None이면 이 창 위가 아니다): 그 점의 놓을 자리를 미리보기로 보인다.
    pub(crate) fn set_dock_preview_from_screen(
        &mut self,
        id: u32,
        screen: Option<(f32, f32)>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let next = screen.and_then(|screen| main_point_preview(screen, screen_bounds(window), &self.dock_slots(), id));
        if next != self.dock_preview {
            self.dock_preview = next;
            cx.notify();
        }
    }

    /// 그리기 앞의 정리: 실제 끌기(gpui)가 끝났으면(창 안에서 자리 없이 놓았거나 밖에서 놓았다) 끌기 상태와 미리보기를 지운다.
    /// 자동 점검이 붙들어 둔 끌기와, 분리 창에서 끌던 중의 미리보기(끌기 상태 없음)는 그대로 둔다.
    pub(crate) fn settle_tab_drag(
        &mut self,
        cx: &Context<Self>,
    ) {
        if self.tab_drag.is_some_and(|state| !state.synthetic) && !cx.has_active_drag() {
            self.tab_drag = None;
            self.dock_preview = None;
        }
        if self.file_drag.is_some() && !cx.has_active_drag() {
            self.file_drag = None;
        }
    }

    /// 창 위에 그릴 끌기 그림: 놓을 자리, 그리고 자동 점검이 붙들어 둔 끌기의 상자.
    pub(crate) fn tab_drag_overlay(&self) -> Vec<Div> {
        let mut parts: Vec<Div> = self.dock_preview.iter().map(dock_preview_view).collect();
        if let Some(state) = self.tab_drag.filter(|state| state.synthetic) {
            let label = self.workspace.name(terminal_of(state.tab)).to_string();
            parts.push(
                div()
                    .absolute()
                    .left(px(state.position.0 + scaled(tab_drag::CURSOR_OFFSET)))
                    .top(px(state.position.1 + scaled(tab_drag::CURSOR_OFFSET)))
                    .child(drag_box(&label, HINT_DETACH)),
            );
        }
        parts
    }

    /// 자동 점검 상태 글의 `dock-preview=` 값: `<방식>:<기준 탭>` 또는 `none`.
    #[cfg(feature = "automation")]
    pub(crate) fn dock_preview_summary(&self) -> String {
        self.dock_preview.map_or("none".to_string(), |preview| preview.summary())
    }

    /// 자동 점검 상태 글의 `drag=` 값: 끌고 있는 터미널 번호 또는 `none`.
    #[cfg(feature = "automation")]
    pub(crate) fn drag_summary(&self) -> String {
        self.tab_drag.map_or("none".to_string(), |state| terminal_of(state.tab).to_string())
    }

    /// 자동 점검 `act dragtab <번호> …`·`act dragback <번호> …`의 합성 좌표 끌기.
    ///
    /// `pane <기준 탭> left|right|top|bottom|center`는 그 탭이 든 칸의 가장자리·가운데 점, `strip <열>`은 그 열 맨 위 칸의 탭 줄 빈 자리 점을
    /// 마우스가 놓은 것과 같은 길(칸 상자 → 놓을 자리 → 옮기기)로 처리한다. `tab <기준 탭> before|after`는 그 탭 앞·뒤에 끼운다.
    /// `outside`는 창 밖 지점에서 뗀 것, `cancel`은 `Esc`를 누르고 창 밖에서 뗀 것(아무 일 없어야 한다)이다.
    /// 맨 앞에 `hold`를 붙이면 놓지 않고 미리보기와 끌기 상자를 그대로 둔다(`act dragtab <번호> cancel`로 푼다).
    /// `dragback`은 같은 점을 분리 창에서 시작한 끌기의 화면 좌표로 바꿔 `dock_from_detached`로 보낸다.
    #[cfg(feature = "automation")]
    pub(crate) fn automation_drag(
        &mut self,
        back: bool,
        args: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::tab_drag_model::{mode_named, point_in};
        let Some(id) = args.first().and_then(|text| text.parse::<u32>().ok()) else { return };
        let dragged = DetachedTab::Terminal(id);
        let viewport = window.viewport_size();
        // 창 밖 지점: 오른쪽으로 40px 벗어난 곳.
        let outside = (f32::from(viewport.width) + 40.0, 120.0);
        let (hold, rest) = match args.get(1..) {
            Some(["hold", rest @ ..]) => (true, rest),
            Some(rest) => (false, rest),
            None => return,
        };
        let slots = self.dock_slots();
        // 합성 좌표: 칸의 가장자리·가운데, 열 맨 위 칸의 탭 줄.
        let point = match rest {
            ["pane", reference, mode] => {
                let slot = reference.parse::<u32>().ok().and_then(|tab| slots.iter().find(|s| s.tabs.contains(&tab)));
                slot.zip(mode_named(mode)).map(|(slot, mode)| point_in(slot.body, mode))
            }
            ["strip", column] => {
                let slot = column.parse::<usize>().ok().and_then(|n| slots.iter().find(|s| s.is_top && s.column == n));
                slot.map(|slot| point_in(slot.strip, paddock_core::tab_drop::DropMode::TabAfter))
            }
            _ => None,
        };
        match rest {
            ["outside"] | ["cancel"] if !back => {
                self.tab_drag = Some(TabDrag { synthetic: true, ..TabDrag::new(dragged, outside) });
                if rest == ["cancel"] {
                    self.cancel_tab_drag(window, cx);
                }
                self.finish_release(outside, window, cx);
            }
            ["tab", reference, mode] if !back => {
                if let (Ok(reference), Some(mode)) = (reference.parse::<u32>(), mode_named(mode)) {
                    if hold {
                        // 끼움 선: 위쪽 탭 줄에 그려진 그 탭의 실제 자리에서 앞(왼쪽 끝 안쪽)·뒤(오른쪽 끝 안쪽)를 가리키는 점으로 정한다.
                        let columns = self.workspace.strip_columns(self.body_size.get().0, self.body_size.get().1);
                        let found = columns.iter().enumerate().find_map(|(column, tabs)| {
                            tabs.iter().position(|(tab, _)| *tab == reference).map(|index| (column, index))
                        });
                        let bounds = found.and_then(|(column, index)| self.tab_scroll(column).bounds_for_item(index));
                        if let Some(bounds) = bounds {
                            let (left, right) = (f32::from(bounds.left()), f32::from(bounds.right()));
                            let x = if mode == paddock_core::tab_drop::DropMode::TabBefore {
                                left + 2.0
                            } else {
                                right - 2.0
                            };
                            self.tab_drag = Some(TabDrag { synthetic: true, ..TabDrag::new(dragged, (x, 200.0)) });
                            self.dock_preview = row_preview(
                                &self.pane_tabs_of(reference),
                                id,
                                RowTab { id: reference, left, right },
                                f32::from(bounds.top()),
                                f32::from(bounds.size.height),
                                x,
                            );
                            cx.notify();
                        }
                    } else {
                        self.dock_tab(id, DockPlace { reference, mode }, window, cx);
                    }
                }
            }
            ["pane", ..] | ["strip", ..] => match point {
                Some((x, y)) if back => {
                    let origin = window.bounds().origin;
                    let screen = (f32::from(origin.x) + x, f32::from(origin.y) + y);
                    if !self.dock_from_detached(id, screen, window, cx) {
                        crate::automation::event(&format!("dragback {id}: no target at {x:.0},{y:.0}"));
                    }
                }
                Some((x, y)) => {
                    let preview = preview_at(&slots, id, x, y);
                    if hold {
                        // 끌기 상자는 창 가장자리에 걸려 잘리지 않게 안쪽으로 당긴다(미리보기 자리는 그대로다).
                        let shown =
                            (x.min(f32::from(viewport.width) - 460.0), y.min(f32::from(viewport.height) - 60.0));
                        self.tab_drag = Some(TabDrag { synthetic: true, ..TabDrag::new(dragged, shown) });
                        self.dock_preview = preview;
                        cx.notify();
                    } else if let Some(preview) = preview {
                        self.dock_tab(id, preview.place, window, cx);
                    } else {
                        crate::automation::event(&format!("dragtab {id}: no target at {x:.0},{y:.0}"));
                    }
                }
                None => crate::automation::event(&format!("dragtab {id}: bad target {rest:?}")),
            },
            _ => crate::automation::event(&format!("dragtab {id}: unknown {rest:?}")),
        }
    }
}
