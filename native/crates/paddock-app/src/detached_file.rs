//! 본 창이 파일 분리 창을 여닫는 동작: 파일 탭을 새 창으로 떼기, 본 창으로 되돌리기, 닫기, 분리 창의 파일 단축키.
//!
//! 파일 탭을 떼는 것은 자리(어느 창)만 옮긴다. 파일 내용(`file_views`)·저장하지 않은 표시(`unsaved`)·편집기 엔티티·디스크 변경 확인은
//! 본 창이 그대로 쥐고, 분리 창은 본 창이 바뀔 때 받아 둔 내용 사본(`FileBodyModel` — 편집기·이미지·PDF 핸들)으로 그린다. 그래서 저장·디스크
//! 변경 띠·묶음 전환은 분리 창과 무관하게 움직인다. 분리 창에서 불린 메서드의 묻기·메뉴·포커스는 본 창에 둔다(`windows.rs`와 같은 규칙).
use std::rc::Rc;

use gpui_kit::{AppContext, Context, Pixels, Point, Window, point, px};
use paddock_core::app_shortcut::FileShortcut;
use paddock_core::file_kind::{FileKind, file_name_of};
use paddock_core::window_point::other_window_point;
use paddock_core::work_model::GroupKey;

use crate::detached_window::{DetachedBody, DetachedShared, DetachedWindow};
use crate::files::pane::FileBodyModel;
use crate::overlay::Overlay;
use crate::tab_drag::screen_bounds;
use crate::window_view::{Active, WorkWindow};
use crate::windows::{DetachedHandle, detached_options, is_offscreen};
use paddock_core::theme::detached_window as detached_size;

/// 자동 점검이 파일 분리 창을 부르는 이름(`keywin`·`shotwin`의 창 번호 자리, `hwnd-<이름>.txt`): `file_` 뒤에 파일 이름.
/// 이 이름이 대본 명령의 한 낱말이고 `shotwin`이 `-`로 가르므로 공백·`-`는 `_`로 바꾼다.
pub fn file_window_name(key: &str) -> String {
    format!("file_{}", file_name_of(key).replace(['-', ' '], "_"))
}

impl WorkWindow {
    /// 묶음 `group` 파일 칸의 `index`번째 탭을 새 창으로 옮긴다. `at`은 새 창 왼쪽 위의 화면 좌표이고, 없으면 본 창에서 (40, 40) 떨어진 자리다.
    ///
    /// 내용이 아직 없으면(보이지 않던 탭은 보일 때 읽는다) 읽기를 시작한다. 창을 열지 못하면 탭을 제자리로 돌려놓는다.
    pub(crate) fn move_file_to_new_window(
        &mut self,
        group: GroupKey,
        index: usize,
        at: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = self.workspace.files().get(&group).and_then(|pane| pane.tabs.get(index)).cloned();
        if let Some(tab) = tab {
            let key = tab.key();
            if !self.file_views.contains_key(&key) {
                let ignore_nul = self.forced_text.contains(&tab.path);
                self.load_tab(tab.path.clone(), tab.kind, ignore_nul, window, cx);
            }
            if self.workspace.detach_file(&group, index).is_some() {
                // 이 묶음의 마지막 파일 탭이 떠나 칸이 사라지면 터미널이 활성이 된다(닫기와 같다).
                if self.workspace.view_key().as_ref() == Some(&group) && self.workspace.files().get(&group).is_none() {
                    self.set_active(Active::Terminal);
                }
                if !self.open_detached_file_window(&key, at, window, cx) {
                    self.workspace.reattach_file(&key);
                    self.status = Some("Could not open a new window.".to_string());
                }
                self.focus_selected(window, cx);
                self.persist();
                cx.notify();
            }
        }
    }

    /// 분리 창을 열고 파일 탭 `key`를 보이게 한다. 열었으면 true다.
    fn open_detached_file_window(
        &mut self,
        key: &str,
        at: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(path) = self.workspace.detached_file(key).map(|detached| detached.tab.path.clone()) else {
            return false;
        };
        let title = file_name_of(&path);
        let shared = Rc::new(DetachedShared::new(&title));
        let origin = at.unwrap_or_else(|| {
            let bounds = window.bounds();
            point(bounds.origin.x + px(detached_size::MENU_OFFSET), bounds.origin.y + px(detached_size::MENU_OFFSET))
        });
        let main = cx.entity();
        let main_window = self.main_window;
        // 창을 만드는 동안 본 창은 갱신 중이라 읽을 수 없으므로, 처음 키 입력을 받을 자리(편집기)와 그릴 내용의 사본을 여기서 건넨다.
        let focus = self.file_views.get(key).and_then(|content| content.focus_handle(cx));
        let file_model = self.workspace.detached_file(key).map(|detached| FileBodyModel::of(self, Some(&detached.tab)));
        let file_key = key.to_string();
        let images = self.graveyard.mirror();
        let result = gpui_kit::open_window(detached_options(&title, origin), cx, |detached_window, cx| {
            cx.new(|cx| {
                DetachedWindow::new(
                    main,
                    main_window,
                    DetachedBody::File { key: file_key.clone() },
                    focus,
                    file_model,
                    Some(images),
                    shared.clone(),
                    detached_window,
                    cx,
                )
            })
        });
        let mut opened = false;
        if let Ok((handle, detached_view)) = result {
            #[cfg(feature = "automation")]
            let _ = handle.update(cx, |_, detached_window, _| {
                crate::platform_keys::note_window_handle(&file_window_name(key), detached_window);
                crate::platform_keys::log_geometry(&format!("detached-{}", file_window_name(key)), detached_window);
            });
            self.detached_file_windows
                .insert(key.to_string(), DetachedHandle { window: handle, view: detached_view, shared });
            opened = true;
        }
        opened
    }

    /// 다른 창에 있던 파일 탭 `key`를 되돌아올 묶음의 파일 칸 끝으로 되돌리고 분리 창을 닫는다. 그 묶음이 보이지 않으면 먼저 보이게 하고,
    /// 되돌아온 탭을 활성으로 만들어 본 창을 앞으로 가져온다. 분리 창을 닫는 길(창 닫기, 메뉴, 끌기)이 모두 이 함수 하나를 지난다.
    pub(crate) fn move_file_back(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((group, _)) = self.workspace.reattach_file(key) {
            self.close_detached_file_window(key, cx);
            self.activate_main(window);
            // 되돌아온 묶음이 지금 보는 묶음이 아니면 그 묶음의 터미널을 골라 보이게 한다(파일 칸은 묶음의 터미널 옆에 붙어 있다).
            if self.workspace.view_key().as_ref() != Some(&group) {
                let shown =
                    self.workspace.work().ids_in(&group).into_iter().find(|id| self.workspace.tab(*id).is_some());
                if let Some(id) = shown {
                    self.workspace.select(id);
                }
            }
            if self.workspace.view_key().as_ref() == Some(&group) {
                self.set_active(Active::File(group));
            }
            self.tab_bar_focused = false;
            self.focus_selected(window, cx);
            self.persist();
            cx.notify();
        }
    }

    /// 파일·폴더의 이름이 `from`에서 `to`로 바뀌었을 때, 그 아래 파일의 분리 창이 새 경로의 탭을 이어서 보이게 한다. 창은 그대로다.
    pub(crate) fn retarget_detached_file_windows(
        &mut self,
        from: &str,
        to: &str,
        cx: &mut Context<Self>,
    ) {
        let old_keys: Vec<String> = self.detached_file_windows.keys().cloned().collect();
        for old in old_keys {
            // 탭 열쇠는 본문 탭이면 경로 그대로이고, 변경 비교 탭이면 `종류\u{1f}경로`다.
            let (prefix, path) = match old.rsplit_once('\u{1f}') {
                Some((prefix, path)) => (Some(prefix), path),
                None => (None, old.as_str()),
            };
            let Some(new_path) =
                paddock_core::paths::rebased(from, to, path).map(|path| paddock_core::file_pane::file_key(&path))
            else {
                continue;
            };
            let new_key = match prefix {
                Some(prefix) => format!("{prefix}\u{1f}{new_path}"),
                None => new_path,
            };
            if let Some(handle) = self.detached_file_windows.remove(&old) {
                let key = new_key.clone();
                handle.view.update(cx, |root, root_cx| root.retarget_file(key, root_cx));
                self.detached_file_windows.insert(new_key, handle);
            }
        }
    }

    /// 분리 창 `key`가 있으면 닫는다(탭을 닫았거나 되돌렸을 때). 그 창이 지금 갱신 중이면 창이 스스로 닫는다.
    pub(crate) fn close_detached_file_window(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
    ) {
        if matches!(&self.overlay, Overlay::CloseDetachedFile(asked) if asked == key) {
            // 그 파일의 닫기 묻기가 떠 있었으면 묻는 대상이 사라졌으니 함께 접는다.
            self.overlay = Overlay::None;
        }
        if let Some(detached) = self.detached_file_windows.remove(key) {
            let _ = detached.window.update(cx, |_, detached_window, _| detached_window.remove_window());
            #[cfg(feature = "automation")]
            crate::automation::note_window_handle(&file_window_name(key), None);
        }
    }

    /// 분리 창의 ×·메뉴 `Close`·`Ctrl+Shift+W`: 본 창을 앞으로 가져오고, 저장하지 않은 변경이 있으면 본 창에 저장 여부 묻기를 연다.
    /// 같은 파일이 다른 탭에도 열려 있으면 편집기를 그쪽 탭이 이어 쥐므로 묻지 않는다.
    pub(crate) fn request_close_detached_file(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asks = self
            .workspace
            .detached_file(key)
            .is_some_and(|detached| self.unsaved.contains(key) && self.n_tabs_of(&detached.tab.path) <= 1);
        if asks {
            self.activate_main(window);
            self.open_overlay(Overlay::CloseDetachedFile(key.to_string()), window, cx);
        } else {
            self.close_detached_file_now(key, window, cx);
        }
    }

    /// 분리 창의 파일 탭을 확인 없이 닫는다: 탭을 지우고 창을 닫고 어느 묶음에도 열려 있지 않은 내용을 비운다.
    pub(crate) fn close_detached_file_now(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.close_detached_file(key) {
            self.close_detached_file_window(key, cx);
            self.prune_file_views();
            self.focus_selected(window, cx);
            self.persist();
            cx.notify();
        }
    }

    /// 분리 창 `key`를 앞으로 가져오고 키 입력을 그 창의 파일 내용에 준다. 화면 밖 자동 점검에서는 창 활성화를 요청만 센다.
    pub(crate) fn activate_detached_file(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
    ) {
        let focus = self.file_views.get(key).and_then(|content| content.focus_handle(cx));
        if let Some(detached) = self.detached_file_windows.get(key) {
            detached.shared.n_activation_requests.set(detached.shared.n_activation_requests.get() + 1);
            let offscreen = is_offscreen();
            let _ = detached.window.update(cx, |_, detached_window, cx| {
                if !offscreen {
                    detached_window.activate_window();
                }
                if let Some(focus) = focus {
                    detached_window.focus(&focus, cx);
                }
            });
        }
    }

    /// 분리 창에서 눌린 파일 단축키(Ctrl+S 저장, Ctrl+G 줄로 가기, Ctrl+Alt+M 검토 메모)를 본 창이 그 창의 파일을 "지금 파일"로 두고 실행한다.
    /// 분리 창이 활성인 파일은 늘 그 파일이므로 활성 탭을 따지지 않는다. 묻기 창을 열면 본 창을 앞으로 가져오고, 그 묶음이 끝날 때까지
    /// `detached_file_context`가 그 파일을 가리킨다. 편집기가 없는 탭(이미지·PDF 등)의 Ctrl+S·Ctrl+G는 가져가지 않아 true가 아니다.
    pub(crate) fn run_detached_file_shortcut(
        &mut self,
        key: &str,
        shortcut: FileShortcut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.workspace.detached_file(key).map(|detached| detached.tab.clone()) else {
            return false;
        };
        let has_editor = !matches!(tab.kind, FileKind::Diff { .. }) && self.text_body_of(&tab.path).is_some();
        self.detached_file_context = Some(key.to_string());
        let handled = match shortcut {
            FileShortcut::Save if has_editor => {
                self.save_active_file(cx);
                true
            }
            FileShortcut::GoToLine if has_editor => self.begin_go_to_line(window, cx),
            FileShortcut::SelectNextOccurrence if has_editor => self.select_next_occurrence(cx),
            FileShortcut::AddReviewNote => {
                self.begin_review_note(window, cx);
                true
            }
            _ => false,
        };
        if self.overlay == Overlay::None {
            self.detached_file_context = None;
        } else {
            self.activate_main(window);
        }
        handled
    }

    /// 분리 창에서 끌어 온 파일 탭 `key`를 화면 좌표 `screen`에 놓았다. 그 점이 본 창 위면 되돌리고 true다(파일 탭은 칸 자리를 고르지 않는다).
    pub(crate) fn dock_file_from_detached(
        &mut self,
        key: &str,
        screen: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let over_main = other_window_point(screen, screen_bounds(window)).is_some();
        if over_main {
            self.move_file_back(key, window, cx);
        }
        over_main
    }

    /// 자동 점검 이름 `name`(`file_<이름>`)이 가리키는 파일 분리 창의 탭 열쇠.
    #[cfg(feature = "automation")]
    pub(crate) fn detached_file_key_named(
        &self,
        name: &str,
    ) -> Option<String> {
        self.detached_file_windows.keys().find(|key| file_window_name(key) == name).cloned()
    }

    /// 자동 점검: 파일 분리 창 `key`에 사람이 창 X를 누른 것과 같은 닫기 메시지를 보낸다(분리 창이 탭을 되돌리고 닫힌다).
    #[cfg(feature = "automation")]
    pub(crate) fn post_close_to_detached_file(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(detached) = self.detached_file_windows.get(key) {
            let _ = detached
                .window
                .update(cx, |_, detached_window, _| crate::platform_keys::post_window_close(detached_window));
        }
    }

    /// 자동 점검: 파일 분리 창 `key`의 크기를 바꾼다(논리 px).
    #[cfg(feature = "automation")]
    pub(crate) fn resize_detached_file(
        &mut self,
        key: &str,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) {
        if let Some(detached) = self.detached_file_windows.get(key) {
            let _ = detached
                .window
                .update(cx, |_, detached_window, _| detached_window.resize(gpui_kit::size(px(width), px(height))));
        }
    }

    /// 자동 점검: 파일 분리 창 `key`의 탭 우클릭 메뉴를 연다.
    #[cfg(feature = "automation")]
    pub(crate) fn open_detached_file_menu(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(detached) = self.detached_file_windows.get(key) {
            detached.view.update(cx, |detached_root, detached_cx| detached_root.open_menu_for_automation(detached_cx));
        }
    }

    /// 자동 점검: 파일 분리 창 `key`의 열린 메뉴에서 이름이 `label`인 항목을 고른 것과 같은 동작을 한다. 메뉴는 닫힌다.
    /// `window`는 본 창이다 — 묻기 창과 활성화는 본 창에서 일어난다.
    #[cfg(feature = "automation")]
    pub(crate) fn run_detached_file_menu_item(
        &mut self,
        key: &str,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(detached) = self.detached_file_windows.get(key) {
            detached.view.update(cx, |detached_root, detached_cx| detached_root.close_menu(detached_cx));
        }
        match label {
            "Move Back to Main Window" => self.move_file_back(key, window, cx),
            "Close" => self.request_close_detached_file(key, window, cx),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_d86_a_file_window_is_named_after_the_file_with_no_dash_or_space() {
        assert_eq!(file_window_name("C:/Temp/proj/b.txt"), "file_b.txt");
        assert_eq!(file_window_name("C:/Temp/proj/sample-3p.pdf"), "file_sample_3p.pdf");
        assert_eq!(file_window_name("/tmp/my notes.md"), "file_my_notes.md");
    }
}
