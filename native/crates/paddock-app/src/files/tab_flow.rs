//! 파일 탭 흐름: 파일을 열어 탭으로 만들고, 작업 스레드가 읽은 내용을 채우고, 디스크 변경을 따라 다시 읽고, 탭을 고르고 닫고, 바깥 프로그램으로 여는 동작.
//!
//! 창의 뿌리 보기(`WorkWindow`)가 쥔 파일 내용(`file_views`)·읽은 시각(`file_stamps`)·줄 이동 대기(`pending_lines`)를 움직이는 쪽만 이 파일에 둔다.
//! 어느 탭이 키를 받는지(`Active`)와 자동 점검 요약은 `window_view`에 남는다.
use std::collections::HashSet;
use std::time::{Duration, Instant};

use gpui_kit::{AppContext, Context, Window};
use paddock_core::file_kind::{FileKind, kind_of};
use paddock_core::file_pane::{file_key, tab_key};
use paddock_core::markdown_view::{View, image_root_for};
use paddock_core::paths::is_inside;
use paddock_core::pen_paths::is_pen_file;
use paddock_core::text_file::Encoded;
use paddock_core::unsaved::{DiskChange, disk_change_effect};
use paddock_core::work_model::GroupKey;

use crate::app_log;
use crate::files::image_view::ImageView;
use crate::files::images::render_image;
use crate::files::markdown_view::{DocumentImages, build_markdown};
use crate::files::open::{
    FileStamp, Loaded, NoticeAction, NoticeReason, file_stamp, load_fit, open_command, read_stable,
};
use crate::files::pane::FileContent;
use crate::files::pdf_view::PdfView;
use crate::files::text_view::{TextFile, build_text, is_editable, scroll_to_line};
use crate::window_view::{Active, Overlay, WorkWindow};
use crate::workspace::TabRef;

/// 열린 파일 탭의 파일이 디스크에서 바뀌었는지 확인하는 주기. 폴더 감시 사건으로 알 수 없는 파일(WSL 공유·네트워크 폴더, 파일 목록이 보여 주지 않는 폴더의 파일)만 이 주기로 본다.
/// 감시하는 폴더 안의 파일은 감시 사건이 올 때 바로 확인한다.
const OPEN_FILE_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// 열린 파일 변경 확인이 이 시간 안에 돌아오지 않으면 멈춘 것으로 보고 새 확인을 허용한다.
const OPEN_FILE_CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// (경로, 종류) 목록에서 같은 항목을 한 번만 남긴다(순서는 처음 나온 대로).
fn unique_files(files: Vec<(String, FileKind)>) -> Vec<(String, FileKind)> {
    let mut unique: Vec<(String, FileKind)> = Vec::new();
    for file in files {
        if !unique.contains(&file) {
            unique.push(file);
        }
    }
    unique
}

/// 읽어 낸 글자와 정보를 편집기에 올릴 텍스트 파일로 묶는다. 고칠 수 있는지는 탭 종류·인코딩·억지로 연 파일인지로 정한다.
fn text_file(
    text: String,
    size: u64,
    encoded: Encoded,
    kind: FileKind,
    forced: bool,
) -> TextFile {
    TextFile { editable: is_editable(kind, &encoded, forced), text, size, encoded }
}

/// 읽는 사이 파일이 바뀌어 읽기를 되풀이하는 최대 횟수.
const READ_ATTEMPTS: usize = 3;

/// 파일을 연 뒤 줄 이동을 다음 그리기마다 다시 시도하는 최대 프레임 수.
const PENDING_LINE_FRAMES: u32 = 30;

impl WorkWindow {
    /// 보는 묶음의 선택한 파일 탭 경로.
    pub(crate) fn selected_file_path(&self) -> Option<String> {
        self.workspace.file_pane().and_then(|pane| pane.tabs.get(pane.selected)).map(|tab| tab.path.clone())
    }

    /// 보는 묶음의 선택한 파일 탭의 열쇠(`file_views`의 열쇠).
    pub(crate) fn selected_file_key(&self) -> Option<String> {
        self.workspace.file_pane().and_then(|pane| pane.tabs.get(pane.selected)).map(|tab| tab.key())
    }

    /// 파일 목록 행·메뉴의 `Open`·대본의 `openfile`: 파일을 열어 활성 탭으로 만든다.
    ///
    /// 탭은 바로 만들고 내용은 작업 스레드가 읽는 대로 채운다(읽는 동안 창이 멈추지 않는다). 이미 열려 있는 파일이면 그 탭을
    /// 고르고 디스크의 지금 내용으로 다시 읽는다.
    ///
    /// `.pen` 파일은 탭을 만들지 않고 Pen 앱에 넘긴다(`open_pen`).
    pub(crate) fn open_file(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // `.pen`은 탭 종류가 아니다. 변경 비교 탭(변경 목록의 Staged·Changes 행)을 뺀 모든 입구가 이 함수를 지나므로 여기서 Pen 앱으로 보낸다.
        // 변경 비교 탭은 `open_file_as`로 바로 열려 `.pen`도 비교 탭으로 열린다.
        if is_pen_file(path) {
            self.open_pen(path, window, cx);
        } else {
            self.open_file_as(path, kind_of(path), window, cx);
        }
    }

    /// `open_file`처럼 파일을 열고, 텍스트 보기가 그려지면 `line`번째(1부터) 줄로 간다. 텍스트가 아닌 파일(이미지·PDF 등)은 줄 번호를 버린다.
    pub(crate) fn open_file_at(
        &mut self,
        path: &str,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_lines.insert(file_key(path), line);
        self.open_file(path, window, cx);
        self.apply_pending_lines(window, cx);
        self.schedule_pending_lines(PENDING_LINE_FRAMES, window, cx);
    }

    /// 줄 이동이 남아 있으면 다음 그리기 직후에 다시 적용해 본다(편집기는 한 번 그려져야 줄 높이를 안다). 그려지기를 기다리는 프레임 수는 `tries`로 제한하고,
    /// 그 뒤에도 남은 이동은 주기 확인(`refresh_agents`)이 이어 받는다.
    fn schedule_pending_lines(
        &mut self,
        tries: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if tries > 0 && !self.pending_lines.is_empty() {
            cx.on_next_frame(window, move |this, window, cx| {
                this.apply_pending_lines(window, cx);
                this.schedule_pending_lines(tries - 1, window, cx);
            });
        }
    }

    /// 기다리던 줄 이동을 적용한다. 텍스트 보기가 아직 한 번도 그려지지 않았거나 내용을 읽는 중이면 다음 확인으로 미루고,
    /// 텍스트 보기가 아니거나 탭이 닫혔으면 버린다.
    pub(crate) fn apply_pending_lines(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: Vec<String> = self.pending_lines.keys().cloned().collect();
        for path in paths {
            let line = self.pending_lines.get(&path).copied().unwrap_or(1);
            let done = match self.file_views.get_mut(&path) {
                Some(FileContent::Text(body)) => scroll_to_line(body, line, window, cx),
                Some(FileContent::Markdown(body)) => {
                    // 줄 번호는 원문의 줄이므로, 미리보기만 보고 있던 탭은 원문 보기로 바꿔 그 줄로 간다(`File`·`Both`는 그대로).
                    if body.view == View::Preview {
                        body.view = View::Source;
                        self.markdown_views.set(&path, View::Source);
                    }
                    body.ensure_source(window, cx);
                    body.source.as_ref().is_some_and(|source| scroll_to_line(source, line, window, cx))
                }
                Some(FileContent::Loading) => false,
                Some(_) => true,
                None => !self.workspace.open_file_keys().any(|open| open == path),
            };
            if done {
                self.pending_lines.remove(&path);
            }
        }
    }

    /// `open_file`과 같되 탭이 보여 주는 방식을 정한다(변경 비교 탭은 `FileKind::Diff`).
    pub(crate) fn open_file_as(
        &mut self,
        path: &str,
        kind: FileKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 같은 파일이 다른 철자(`C:\x`·`C:/x`·`..` 포함)로 와도 한 탭이 되도록, 이 입구에서 경로를 한 가지 철자로 맞춘 뒤 모든 곳에서 그 철자를 쓴다.
        let path = file_key(path);
        let path = path.as_str();
        // 이미 다른 창(파일 분리 창)에 열려 있는 탭이면 같은 탭을 또 만들지 않고 그 창을 앞으로 가져온다.
        let wanted = tab_key(path, kind);
        if self.workspace.is_file_detached(&wanted) {
            self.activate_detached_file(&wanted, cx);
            cx.notify();
        } else if let Some((key, index)) = self.workspace.open_file(path, kind) {
            let kind = self.workspace.files().get(&key).and_then(|pane| pane.tabs.get(index)).map(|tab| tab.kind);
            let ignore_nul = self.forced_text.contains(path);
            self.load_tab(path.to_string(), kind.unwrap_or(FileKind::Text), ignore_nul, window, cx);
            if self.workspace.view_key().as_ref() == Some(&key) {
                self.set_active(Active::File(key));
            }
            self.focus_selected(window, cx);
            self.persist();
            cx.notify();
        }
    }

    /// 보이는 묶음에서 선택한 파일 탭의 내용이 아직 없으면 읽기를 시작한다. 보이지 않는 묶음이나 선택하지 않은 탭은 보일 때 읽는다
    /// (앱을 다시 켜도 열어 둔 모든 파일을 한꺼번에 읽지 않는다). 읽기를 시작했으면 true다.
    pub(crate) fn load_visible_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // 보이는 묶음의 선택한 탭과, 파일 분리 창에 있는(늘 보이는) 탭 중 내용이 없는 것.
        let wanted = self
            .workspace
            .file_pane()
            .and_then(|pane| pane.tabs.get(pane.selected))
            .into_iter()
            .chain(self.workspace.detached_files().iter().map(|detached| &detached.tab))
            .find(|tab| !self.file_views.contains_key(&tab.key()))
            .map(|tab| (tab.path.clone(), tab.kind));
        let started = wanted.is_some();
        if let Some((path, kind)) = wanted {
            let ignore_nul = self.forced_text.contains(&path);
            self.load_tab(path, kind, ignore_nul, window, cx);
        }
        started
    }

    /// 탭 종류에 맞는 읽기를 시작한다: 변경 비교 탭은 git에게, 나머지는 파일을 읽는 작업 스레드에게 시킨다.
    pub(crate) fn load_tab(
        &mut self,
        path: String,
        kind: FileKind,
        ignore_nul: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match kind {
            FileKind::Diff { staged } => self.start_diff_load(path, staged),
            _ => self.start_file_load(path, kind, ignore_nul, window, cx),
        }
    }

    /// 파일을 작업 스레드에서 읽어 내용을 만든다. 읽는 동안 이미 보이던 내용이 있으면 그대로 두고, 없으면 빈 자리를 보인다.
    /// `ignore_nul`은 사용자가 `Open as text anyway`로 고른 경우다. 이미지는 지금 창 크기 안으로 줄여 올린다(1:1로 바꿀 때 원본을 다시 읽는다).
    pub(crate) fn start_file_load(
        &mut self,
        path: String,
        kind: FileKind,
        ignore_nul: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_views.entry(path.clone()).or_insert(FileContent::Loading);
        let viewport = window.viewport_size();
        let scale = window.scale_factor();
        let bound = ((f32::from(viewport.width) * scale) as u32, (f32::from(viewport.height) * scale) as u32);
        cx.spawn_in(window, async move |this, cx| {
            let (loaded, stamp) = {
                let path = path.clone();
                cx.background_executor()
                    .spawn(async move {
                        // 읽기 전에 쓴 시각을 먼저 잰다. 읽는 사이 파일이 바뀌었으면 그 결과를 버리고 다시 읽는다(최대 세 번).
                        // 끝내 바뀌는 중이면 읽기 전 시각을 남겨 다음 확인이 다시 읽게 한다.
                        let (loaded, stamp) = read_stable(
                            || file_stamp(&path),
                            || load_fit(&path, kind, ignore_nul, Some(bound)),
                            READ_ATTEMPTS,
                        );
                        (loaded, stamp)
                    })
                    .await
            };
            let _ = this.update_in(cx, |this, window, cx| this.finish_file_load(path, kind, loaded, stamp, window, cx));
        })
        .detach();
    }

    /// 파일이 속한 작업 폴더(Markdown 속 이미지를 읽을 수 있는 범위). 어느 작업 폴더에도 없으면 파일이 있는 폴더다.
    fn work_folder_of_file(
        &self,
        path: &str,
    ) -> String {
        image_root_for(self.workspace.work(), path)
    }

    /// 읽기 결과로 파일 탭의 내용을 바꾼다. 읽는 사이 탭이 닫혔으면 버린다. `kind`는 읽기를 시작할 때 탭이 보여 주기로 한 방식이다.
    /// 같은 종류의 내용이 이미 보이고 있으면 새로 만들지 않고 그 안의 내용만 바꿔 스크롤 위치·배율·보기 방식을 지킨다.
    fn finish_file_load(
        &mut self,
        path: String,
        kind: FileKind,
        loaded: Loaded,
        stamp: Option<FileStamp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_open = self.workspace.open_file_keys().any(|open| open == path);
        let loaded_key = tab_key(&path, kind);
        // 저장하지 않은 편집이 있는 텍스트는 읽은 내용으로 덮지 않는다(다시 연 파일·감시가 읽은 파일 모두). 디스크가 열었을 때와 다르면 띠가 `Reload`·`Keep mine`을 묻는다.
        let effect =
            disk_change_effect(self.unsaved.contains(&path), &self.file_stamps.get(&path).copied().flatten(), &stamp);
        // 읽은 결과가 텍스트가 아니어도(이진·크기 초과·읽기 실패로 바뀜) 고친 내용은 버리지 않고 같은 띠로 묻는다.
        let keeps_edits = is_open
            && effect != DiskChange::Reload
            && matches!(self.file_views.get(&path), Some(FileContent::Text(_) | FileContent::Markdown(_)));
        if keeps_edits {
            if effect == DiskChange::Ask {
                self.disk_changed.insert(path);
            }
            cx.notify();
        } else if is_open {
            self.file_stamps.insert(path.clone(), stamp);
            let forced = self.forced_text.contains(&path);
            let root = self.work_folder_of_file(&path);
            let graveyard = self.graveyard.clone();
            let view = self.markdown_views.get(&path);
            let replaced = match (self.file_views.get_mut(&path), loaded) {
                (Some(FileContent::Text(body)), Loaded::Text { text, size, encoded }) if kind != FileKind::Markdown => {
                    body.replace(text_file(text, size, encoded, kind, forced), window, cx);
                    None
                }
                (Some(FileContent::Markdown(body)), Loaded::Text { text, size, encoded })
                    if kind == FileKind::Markdown =>
                {
                    body.replace(text_file(text, size, encoded, kind, forced), window, cx);
                    None
                }
                (Some(FileContent::Image(existing)), Loaded::Image { pixels, original, size }) => {
                    match render_image(pixels) {
                        Some(image) => {
                            existing.update(cx, |existing, cx| existing.replace(image, original, size, cx));
                            None
                        }
                        None => Some(FileContent::Notice { reason: NoticeReason::ImageUnreadable, size }),
                    }
                }
                (Some(FileContent::Pdf(existing)), Loaded::Pdf { document, size }) => {
                    existing.update(cx, |existing, cx| existing.replace(document, size, cx));
                    None
                }
                (_, loaded) => Some(match loaded {
                    Loaded::Text { text, size, encoded } if kind == FileKind::Markdown => {
                        FileContent::Markdown(build_markdown(
                            text_file(text, size, encoded, kind, forced),
                            &path,
                            DocumentImages::new(graveyard, root),
                            view,
                            window,
                            cx,
                        ))
                    }
                    Loaded::Text { text, size, encoded } => {
                        FileContent::Text(build_text(text_file(text, size, encoded, kind, forced), &path, window, cx))
                    }
                    Loaded::Image { pixels, original, size } => match render_image(pixels) {
                        Some(image) => FileContent::Image(
                            cx.new(|_| ImageView::new(path.clone(), image, original, size, graveyard)),
                        ),
                        None => FileContent::Notice { reason: NoticeReason::ImageUnreadable, size },
                    },
                    Loaded::Pdf { document, size } => {
                        FileContent::Pdf(cx.new(|_| PdfView::new(document, size, graveyard)))
                    }
                    Loaded::Notice { reason, size } => FileContent::Notice { reason, size },
                    Loaded::Failed(message) => FileContent::Failed(message),
                }),
            };
            if let Some(content) = replaced {
                self.file_views.insert(path, content);
            }
            self.apply_pending_lines(window, cx);
            self.schedule_pending_lines(PENDING_LINE_FRAMES, window, cx);
            // 묻기 창이 떠 있으면 늦게 도착한 내용이 포커스를 가져가지 않는다. 사용자가 그 사이 파일 목록 행을 눌러 목록이 키를 받고 있으면
            // 그것도 뺏지 않는다(처음 여는 파일은 읽기가 늦게 끝나므로, 안 그러면 F2·Ctrl+V가 눌러 둔 행이 아니라 터미널로 간다).
            // 읽은 탭이 본 창 칸에서 선택된 탭일 때만이다(파일 분리 창의 탭이면 본 창 편집기 포커스가 그 창에 가지 않게 한다).
            let on_screen = self.selected_file_key().is_some_and(|selected| selected == loaded_key);
            if on_screen && self.file_active() && self.overlay == Overlay::None && !self.files.list_focused() {
                self.focus_selected(window, cx);
            }
            cx.notify();
        }
    }

    /// 열린 파일 탭의 파일이 디스크에서 바뀌었는지 작업 스레드에서 쓴 시각·크기로 보고, 바뀐 파일을 다시 읽는다.
    /// 지금 보이는 탭(보는 묶음에서 선택한 탭)만 바로 다시 읽고, 나머지는 내용을 비워 두었다가 보일 때 읽는다.
    ///
    /// 감시하는 폴더 안의 파일은 감시 사건이 올 때(`n_events`가 달라질 때) 바로 확인하고, 사건으로 알 수 없는 파일(WSL 공유·네트워크 폴더, 파일 목록이 보여 주지 않는
    /// 폴더의 파일)만 `OPEN_FILE_CHECK_INTERVAL`마다 확인한다. `listing_changed`는 주기 비교(WSL 목록)가 폴더 안 변경을 봤다는 뜻이라 모든 열린 파일을 확인한다.
    /// 쓴 시각을 읽지 못한 파일(지워졌거나 잠김)은 마지막 내용을 그대로 둔다.
    pub(crate) fn watch_open_files(
        &mut self,
        listing_changed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let due = self.open_files_checked_at.is_none_or(|at| at.elapsed() >= OPEN_FILE_CHECK_INTERVAL);
        let events = self.files.n_events();
        let events_seen = events != self.open_files_events_seen;
        let check_all = listing_changed || events_seen;
        // 확인이 끝나 돌아오지 않으면(느린 공유 폴더가 응답하지 않는 등) 영영 멈추지 않도록 일정 시간 뒤 새 확인을 허용한다. 그때 옛 확인의 결과는 버린다.
        let stuck = self.open_files_check_started.is_some_and(|at| at.elapsed() >= OPEN_FILE_CHECK_TIMEOUT);
        if (check_all || due) && (!self.open_files_checking || stuck) {
            self.open_files_events_seen = events;
            let watched = self.files.events_folder().map(str::to_string);
            let open: Vec<(String, FileKind)> = self
                .workspace
                .file_tabs()
                .filter(|tab| !matches!(tab.kind, FileKind::Diff { .. } | FileKind::Media | FileKind::Binary))
                // 저장 중인 파일(자기 쓰기를 변경으로 오인하지 않게)과 이미 띠로 알린 파일은 다시 확인하지 않는다.
                .filter(|tab| !self.saving.contains(&tab.path) && !self.disk_changed.contains(&tab.path))
                .filter(|tab| matches!(self.file_views.get(&tab.key()), Some(content) if !matches!(content, FileContent::Loading)))
                // 주기 확인만 도는 때는 사건으로 알 수 있는 파일(감시하는 폴더 안)을 건너뛴다.
                .filter(|tab| check_all || !watched.as_deref().is_some_and(|folder| is_inside(folder, &tab.path)))
                .map(|tab| (tab.path.clone(), tab.kind))
                .collect();
            // 같은 파일이 두 묶음에 열려 있으면 탭이 둘이지만 확인과 다시 읽기는 한 번이다.
            let open = unique_files(open);
            self.open_files_checked_at = Some(Instant::now());
            if !open.is_empty() {
                self.open_files_checking = true;
                self.open_files_check_started = Some(Instant::now());
                self.open_files_check_generation += 1;
                let generation = self.open_files_check_generation;
                let known = self.file_stamps.clone();
                cx.spawn_in(window, async move |this, cx| {
                    let changed: Vec<(String, FileKind)> = cx
                        .background_executor()
                        .spawn(async move {
                            open.into_iter()
                                .filter(|(path, _)| {
                                    let now = file_stamp(path);
                                    now.is_some() && known.get(path).is_some_and(|before| *before != now)
                                })
                                .collect()
                        })
                        .await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        // 더 새 확인이 시작됐으면(이 확인이 너무 오래 걸렸다) 이 결과는 버린다.
                        if this.open_files_check_generation == generation {
                            this.open_files_checking = false;
                            this.open_files_check_started = None;
                            for (path, kind) in unique_files(changed) {
                                this.note_disk_change(path, kind, generation, window, cx);
                            }
                        }
                    });
                })
                .detach();
            }
        }
    }

    /// 변경 확인(세대 `generation`)이 `path`가 바뀌었다고 알려 왔다. 그 확인이 자기 쓰기가 끝나기 전에 시작된 것이면 자기 쓰기를 본 것일 수 있어 버린다.
    /// 고친 내용이 있으면 덮지 않고 띠로 묻고, 보이는 탭이면 바로 다시 읽고, 아니면 내용을 비워 보일 때 읽게 한다.
    pub(crate) fn note_disk_change(
        &mut self,
        path: String,
        kind: FileKind,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let own_write = self.own_write_generation.get(&path).is_some_and(|written| *written >= generation);
        if !own_write {
            // 다른 창(파일 분리 창)의 탭은 늘 보이는 탭이다.
            let key = tab_key(&path, kind);
            let visible = self.selected_file_key().as_ref() == Some(&key) || self.workspace.is_file_detached(&key);
            if self.unsaved.contains(&path) {
                // 고친 내용을 읽은 내용으로 덮지 않는다: 띠가 `Reload`·`Keep mine`을 묻는다.
                self.disk_changed.insert(path);
                cx.notify();
            } else if visible {
                let ignore_nul = self.forced_text.contains(&path);
                self.start_file_load(path, kind, ignore_nul, window, cx);
            } else {
                // 보이지 않는 탭은 지금 읽지 않는다: 내용을 비워 두면 그 탭이 보일 때 새로 읽는다(그동안 메모리도 비운다).
                self.file_views.remove(&path);
                self.file_stamps.remove(&path);
            }
        }
    }

    /// 어느 묶음에도 열려 있지 않은 파일의 내용을 버린다.
    pub(crate) fn prune_file_views(&mut self) {
        let open: HashSet<String> = self.workspace.open_file_keys().collect();
        self.file_views.retain(|key, _| open.contains(key));
        self.file_stamps.retain(|path, _| open.contains(path));
        self.forced_text.retain(|path| open.contains(path));
        self.unsaved.retain(|path| open.contains(path));
        self.own_write_generation.retain(|path, _| open.contains(path));
        self.disk_changed.retain(|path| open.contains(path));
    }

    /// 탭 줄의 파일 탭을 누르면 그 탭을 고르고 키를 파일 쪽으로 보낸다.
    pub(crate) fn select_file_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tab_bar_focused = false;
        self.activate_tab(TabRef::File(index));
        self.focus_selected(window, cx);
        self.persist();
        cx.notify();
    }

    /// 파일 칸을 누르면 파일 탭이 활성이 되어 키가 파일 쪽으로 간다.
    pub(crate) fn activate_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.file_active()
            && let Some(key) = self.workspace.view_key()
            && self.workspace.files().get(&key).is_some()
        {
            self.set_active(Active::File(key));
        }
        self.focus_selected(window, cx);
        cx.notify();
    }

    /// 파일 탭을 확인 없이 닫는다(저장 여부 묻기는 `close_file_tab`이 한다). 칸의 탭이 모두 닫히면 터미널이 다시 활성이 된다.
    pub(crate) fn close_file_tab_now(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(key) = self.workspace.view_key() {
            self.close_file_tab_in(key, index, window, cx);
        }
    }

    /// 묶음 `key`의 파일 칸에서 `index`번째 탭을 확인 없이 닫는다. 지금 보는 묶음이 아니면 활성 탭·포커스는 건드리지 않는다.
    pub(crate) fn close_file_tab_in(
        &mut self,
        key: GroupKey,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.close_file(&key, index) {
            let shown = self.workspace.view_key().as_ref() == Some(&key);
            if shown && self.workspace.files().get(&key).is_none() {
                self.set_active(Active::Terminal);
            }
            self.prune_file_views();
            if shown {
                self.focus_selected(window, cx);
            }
            self.persist();
            cx.notify();
        }
    }

    /// 안내 탭의 단추를 실행한다.
    pub(crate) fn run_notice_action(
        &mut self,
        action: NoticeAction,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            NoticeAction::ShowInFileManager => self.reveal_in_system(path),
            NoticeAction::OpenAsText => {
                if let Some(key) = self.detached_file_key_of(path) {
                    // 파일 분리 창의 안내 탭이다.
                    self.workspace.set_detached_file_kind(&key, FileKind::Text);
                    self.forced_text.insert(path.to_string());
                    self.start_file_load(path.to_string(), FileKind::Text, true, window, cx);
                } else if let Some(key) = self.workspace.view_key()
                    && let Some(index) = self
                        .workspace
                        .files()
                        .get(&key)
                        .and_then(|pane| pane.tabs.iter().position(|tab| tab.path == path))
                {
                    self.workspace.set_file_kind(&key, index, FileKind::Text);
                    self.forced_text.insert(path.to_string());
                    self.start_file_load(path.to_string(), FileKind::Text, true, window, cx);
                }
            }
            NoticeAction::OpenWithDefaultApp => {
                // 너무 큰 텍스트는 편집기로, 이미지·PDF·오디오·영상은 기본 앱으로 연다.
                let as_text = matches!(
                    self.file_views.get(path),
                    Some(FileContent::Notice { reason: NoticeReason::TooLarge, .. })
                );
                self.open_externally(path, as_text);
            }
        }
        cx.notify();
    }

    /// 보는 묶음의 선택한 파일 탭이 보여 주는 방식.
    pub(crate) fn selected_file_kind(&self) -> Option<FileKind> {
        self.workspace.file_pane().and_then(|pane| pane.tabs.get(pane.selected)).map(|tab| tab.kind)
    }

    /// 선택한 파일 탭이 Markdown이면 지금 보이는 방식.
    pub(crate) fn selected_markdown_view(&self) -> Option<View> {
        match self.selected_file_key().and_then(|key| self.file_views.get(&key)) {
            Some(FileContent::Markdown(body)) => Some(body.view),
            _ => None,
        }
    }

    /// 탭 줄의 `File`·`Preview`·`Both`: 선택한 Markdown 탭의 보기 방식을 바꾸고 파일별로 기억한다. 키는 계속 파일 칸이 받는다.
    pub(crate) fn set_markdown_view(
        &mut self,
        view: View,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let (Some(path), Some(key)) = (self.selected_file_path(), self.selected_file_key())
            && self.selected_file_kind() == Some(FileKind::Markdown)
        {
            self.markdown_views.set(&path, view);
            if let Some(FileContent::Markdown(body)) = self.file_views.get_mut(&key) {
                body.view = view;
                if view != View::Preview {
                    body.ensure_source(window, cx);
                }
            }
            self.activate_file(window, cx);
        }
    }

    /// 탭 줄의 `−`·`+`: 선택한 PDF 탭의 배율을 한 단계 바꾼다. `None`은 칸 폭 맞춤으로 되돌린다.
    pub(crate) fn zoom_pdf(
        &mut self,
        step: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(FileContent::Pdf(view)) = self.selected_file_key().and_then(|key| self.file_views.get(&key)) {
            view.update(cx, |view, cx| match step {
                Some(true) => view.zoom_in(cx),
                Some(false) => view.zoom_out(cx),
                None => view.zoom_fit(cx),
            });
            self.activate_file(window, cx);
        }
    }

    /// 탭 줄의 `Open with default app`: 선택한 PDF 파일을 기본 앱에 맡긴다.
    pub(crate) fn open_active_file_with_default_app(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.selected_file_path() {
            self.open_externally(&path, false);
        }
        cx.notify();
    }

    /// 탭 줄 오른쪽의 `Open in external editor`: 활성 파일 탭의 파일을 바깥 편집기로 연다.
    pub(crate) fn open_active_file_externally(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.selected_file_path() {
            // 단추는 글자로 읽는 종류에만 보이지만, 명령으로 와도 같은 조건으로 편집기/기본 앱을 가른다.
            let as_text = self.selected_file_kind().is_some_and(FileKind::opens_in_text_editor);
            self.open_externally(&path, as_text);
        }
        cx.notify();
    }

    /// 파일을 바깥 프로그램으로 연다. 셸을 거치지 않고 인자 배열로 띄우며 콘솔 창을 만들지 않는다.
    fn open_externally(
        &mut self,
        path: &str,
        as_text: bool,
    ) {
        let (program, args) = open_command(std::env::consts::OS, path, as_text);
        // Windows의 기본 앱 열기는 명령줄을 거치지 않고 셸에 경로를 그대로 넘긴다(쉼표·공백·한글이 든 경로 때문).
        #[cfg(windows)]
        let handed_to_shell = program == "explorer.exe";
        #[cfg(not(windows))]
        let handed_to_shell = false;
        if handed_to_shell {
            #[cfg(windows)]
            {
                let path = path.to_string();
                std::thread::spawn(move || {
                    if let Err(reason) = crate::files::open::shell_open(&path) {
                        app_log::event(&format!("could not open a file with the default app: {reason}"));
                    }
                });
            }
        } else {
            self.spawn_external(&program, &args, "open the file");
        }
    }
}
