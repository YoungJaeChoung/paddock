//! 파일 목록의 창 쪽 상태: 지금 보여 주는 폴더, 펼친 폴더, 마지막 읽기 결과(행). 읽기는 `FileService`가 맡는다.
use std::collections::HashSet;
use std::sync::mpsc::TryRecvError;

use paddock_core::file_tree::{
    FileRow, ListFocusEvent, creation_folder, hidden_names, list_focus_after, move_expanded, rows,
};
use paddock_core::paths::rebased;

use super::{FileService, Request, STOPPED_NOTICE};
use crate::settings::Settings;

/// 창이 쥔 파일 목록 상태.
pub struct FilesState {
    /// 첫 폴더를 보여 줄 때 시작한다. 목록이 한 번도 보이지 않은 실행에는 스레드를 만들지 않는다.
    service: Option<FileService>,
    /// 목록이 가리키는 폴더. 사이드바가 접혀 있어도 이 폴더와 펼친 폴더를 기억한다.
    folder: Option<String>,
    /// 읽고 감시하는 폴더. 목록이 보일 때만 `folder`와 같고, 가려지면 None이다.
    watched: Option<String>,
    /// 펼친 폴더의 경로. 경로가 절대 경로라 다른 폴더를 보고 돌아와도 그대로 펼쳐져 있다.
    pub expanded_dirs: HashSet<String>,
    /// 마지막으로 펼친 폴더. 새 파일·새 폴더가 이 안에 만들어진다.
    pub last_expanded: Option<String>,
    /// 머리줄의 접기 표지 상태. 접으면 폴더를 읽지도 감시하지도 않는다.
    pub files_expanded: bool,
    hidden: HashSet<String>,
    rows: Vec<FileRow>,
    /// `rows`를 읽어 온 폴더. `folder`와 다르면 아직 이 폴더의 읽기 결과가 오지 않은 것이다.
    loaded_folder: Option<String>,
    /// 폴더를 파일 사건 대신 주기 비교로 지켜보는 중이다.
    polled: bool,
    /// 창이 가려져(최소화) 서비스의 주기 비교를 멈춘 상태.
    paused: bool,
    /// 서비스가 지금까지 폴더를 디스크에서 실제로 읽은 횟수(마지막 결과 기준).
    n_listings: usize,
    /// 마지막 읽기를 창 스레드가 아닌 곳에서 했다.
    read_off_window: bool,
    /// 마지막 읽기에 걸린 시간(밀리초). 느린 파일 시스템에서 읽기가 얼마나 걸리는지 보려는 진단 값이다.
    last_read_ms: u128,
    /// 주기 비교(WSL 공유)가 이미 읽어 둔 폴더에서 이름 목록이 달라진 것을 봤다. 폴더 안 변경이 있었다는 신호로 `take_change_seen`이 꺼내 간다.
    change_seen: bool,
    /// 선택한 행의 경로(행을 누르면 그 행). F2 이름 바꾸기와 Ctrl+V 붙여넣기가 가리키는 행이다.
    selected_row: Option<String>,
    /// 목록이 키를 받는 구역이다(행을 누른 뒤 다른 구역을 누르거나 터미널·편집기로 키가 가기 전까지). 키를 받는 쪽(`focus_selected`)을 되돌리는 곳이 거짓으로 만든다.
    list_focused: bool,
    /// 읽기 작업 스레드가 오류로 사라졌을 때 목록 자리에 보일 안내. 다음 요청(새로 고침·폴더 바꾸기)이 새 스레드를 시작하며 지운다.
    failure: Option<String>,
}

impl Default for FilesState {
    fn default() -> Self {
        Self {
            service: None,
            folder: None,
            watched: None,
            expanded_dirs: HashSet::new(),
            last_expanded: None,
            files_expanded: true,
            hidden: HashSet::new(),
            rows: Vec::new(),
            loaded_folder: None,
            polled: false,
            paused: false,
            n_listings: 0,
            read_off_window: false,
            last_read_ms: 0,
            change_seen: false,
            selected_row: None,
            list_focused: false,
            failure: None,
        }
    }
}

impl FilesState {
    /// 지금 보여 주는 폴더.
    pub fn folder(&self) -> Option<&str> {
        self.folder.as_deref()
    }

    /// 화면에 그릴 행. 아직 읽기 결과가 없으면 비어 있다.
    pub fn rows(&self) -> &[FileRow] {
        &self.rows
    }

    /// 보이는 폴더의 읽기 결과가 도착했다.
    pub fn is_loaded(&self) -> bool {
        self.watched.is_some() && self.loaded_folder == self.watched
    }

    /// 폴더를 디스크에서 실제로 읽은 횟수. 수정 시각이 그대로인 폴더는 다시 읽지 않으므로 다시 읽기 요청보다 적을 수 있다.
    pub fn n_listings(&self) -> usize {
        self.n_listings
    }

    /// 마지막 읽기를 창 스레드가 아닌 곳에서 했는지.
    pub fn read_off_window(&self) -> bool {
        self.read_off_window
    }

    /// 마지막 읽기에 걸린 시간(밀리초).
    pub fn last_read_ms(&self) -> u128 {
        self.last_read_ms
    }

    /// 주기 비교가 폴더 안 변경을 본 적이 있는지를 꺼내며 지운다. 처음 읽은 결과와 다른 폴더로 옮긴 직후의 결과는 변경이 아니다.
    pub fn take_change_seen(&mut self) -> bool {
        std::mem::take(&mut self.change_seen)
    }

    /// 읽기 작업 스레드가 멈춰 목록을 읽지 못할 때의 안내.
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// 감시가 폴더 안 변경을 알린 횟수(서비스가 없으면 0). 열린 파일의 변경 확인이 이 값이 달라졌을 때 바로 돈다.
    pub fn n_events(&self) -> usize {
        self.service.as_ref().map_or(0, FileService::n_events)
    }

    /// 파일 사건으로 지켜보고 있는 폴더(읽기 결과가 왔고 주기 비교로 내려가지 않은 것). 이 폴더 아래 파일의 변경은 사건으로 알 수 있다.
    pub fn events_folder(&self) -> Option<&str> {
        self.watched.as_deref().filter(|_| self.is_loaded() && !self.polled)
    }

    /// 폴더를 파일 사건 대신 주기 비교로 지켜보는 중인지(마지막 읽기 기준).
    pub fn is_polled(&self) -> bool {
        self.polled
    }

    /// 목록이 가리킬 폴더와 지금 보이는지를 정한다. 폴더가 바뀌면 이전 폴더의 행을 비운다.
    /// 읽고 감시하는 대상(폴더가 있고 보일 때)이 바뀌면 서비스에 새 읽기를 요청한다. 무엇이든 바뀌었으면 true다.
    pub fn show(
        &mut self,
        folder: Option<String>,
        visible: bool,
    ) -> bool {
        let folder_changed = folder != self.folder;
        if folder_changed {
            self.folder = folder;
            self.rows.clear();
            self.loaded_folder = None;
        }
        let watched = self.folder.clone().filter(|_| visible);
        let watch_changed = watched != self.watched;
        if watch_changed {
            self.watched = watched;
            if self.watched.is_some() {
                self.hidden = hidden_names(Settings::load().values());
            }
            self.send(false);
        }
        folder_changed || watch_changed
    }

    /// 창이 보이는지 알린다. 가려진 동안 WSL 공유의 주기 비교를 멈추고, 다시 보이면 한 번 바로 확인한다.
    /// 창이 비활성이어도 보이면 멈추지 않는다(다른 창을 쓰며 에이전트 작업을 지켜볼 수 있다).
    pub fn set_window_visible(
        &mut self,
        visible: bool,
    ) {
        if self.paused == visible {
            self.paused = !visible;
            if let Some(service) = &self.service {
                service.set_paused(self.paused);
            }
        }
    }

    /// 행을 눌렀다: 그 행을 선택하고 목록이 키를 받는다.
    pub fn select_row(
        &mut self,
        path: &str,
    ) {
        self.selected_row = Some(path.to_string());
        self.focus_event(ListFocusEvent::RowPressed);
    }

    /// 목록이 키를 받는지를 바꾸는 사건을 알린다. 규칙은 `list_focus_after` 하나다.
    pub fn focus_event(
        &mut self,
        event: ListFocusEvent,
    ) {
        self.list_focused = list_focus_after(event);
    }

    /// 목록이 키를 받고 있는지.
    pub fn list_focused(&self) -> bool {
        self.list_focused
    }

    /// 지금 보이는 선택한 행의 경로. 선택한 행이 없거나 목록에서 사라졌으면 None이다.
    pub fn selected_row(&self) -> Option<&str> {
        self.selected_row
            .as_deref()
            .filter(|path| self.rows.iter().any(|row| row.path == *path && row.notice.is_none()))
    }

    /// 키를 받고 있는 선택 행(경로, 폴더인지). F2·Ctrl+V가 가리키는 대상이다.
    pub fn focused_entry(&self) -> Option<(String, bool)> {
        self.selected_row()
            .filter(|_| self.list_focused())
            .and_then(|path| self.rows.iter().find(|row| row.path == path).map(|row| (row.path.clone(), row.is_dir)))
    }

    /// 이름이 `from`에서 `to`로 바뀐 뒤: 선택 행과 펼친 폴더 기억을 새 경로로 옮기고 읽은 목록을 버려 다시 읽는다.
    pub fn renamed(
        &mut self,
        from: &str,
        to: &str,
    ) {
        let (expanded, last) = move_expanded(&self.expanded_dirs, self.last_expanded.as_deref(), from, to);
        self.expanded_dirs = expanded;
        self.last_expanded = last;
        self.selected_row =
            self.selected_row.as_deref().map(|path| rebased(from, to, path).unwrap_or_else(|| path.to_string()));
        self.send(true);
    }

    /// 폴더 행을 접거나 펼친다. 펼치면 그 폴더가 새 파일·새 폴더의 기본 자리가 된다.
    pub fn toggle_dir(
        &mut self,
        path: &str,
    ) {
        if self.expanded_dirs.remove(path) {
            if self.last_expanded.as_deref() == Some(path) {
                self.last_expanded = None;
            }
        } else {
            self.expanded_dirs.insert(path.to_string());
            self.last_expanded = Some(path.to_string());
        }
        self.send(false);
    }

    /// 읽어 둔 목록을 모두 버리고 디스크에서 다시 읽는다. 숨길 이름 설정도 다시 읽는다.
    pub fn refresh(&mut self) {
        self.hidden = hidden_names(Settings::load().values());
        self.send(true);
    }

    /// 새 항목이 보이도록 그 부모 폴더를 펼치고 다시 읽는다. 새로 만든 것이 폴더(`new_folder`)면 그 폴더도 펼친다.
    pub fn show_added(
        &mut self,
        parent: &str,
        new_folder: Option<&str>,
    ) {
        self.expanded_dirs.insert(parent.to_string());
        self.expanded_dirs.extend(new_folder.map(str::to_string));
        self.files_expanded = true;
        self.send(true);
    }

    /// 머리줄의 새 파일·새 폴더가 만들 폴더. 목록이 가리키는 폴더가 없으면 None이다.
    pub fn creation_folder(&self) -> Option<String> {
        let folder = self.folder.as_deref()?;
        let visible_open: HashSet<String> =
            self.rows.iter().filter(|row| row.is_dir && row.expanded).map(|row| row.path.clone()).collect();
        Some(creation_folder(self.last_expanded.as_deref(), &visible_open, folder))
    }

    /// 도착한 읽기 결과를 반영한다. 행이 바뀌었으면 true다.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let mut drained = false;
        while !drained && let Some(service) = self.service.as_ref() {
            match service.try_recv() {
                Ok(snapshot) => {
                    if self.watched.as_deref() == Some(snapshot.folder.as_str()) {
                        let next = rows(&snapshot.directory);
                        let was_loaded = self.loaded_folder.as_deref() == Some(snapshot.folder.as_str());
                        self.change_seen |= snapshot.polled && was_loaded && next != self.rows;
                        self.rows = next;
                        self.loaded_folder = Some(snapshot.folder);
                        self.polled = snapshot.polled;
                        self.last_read_ms = snapshot.elapsed.as_millis();
                        self.n_listings = snapshot.n_listings;
                        self.read_off_window = snapshot.read_thread != std::thread::current().id();
                        changed = true;
                    }
                }
                Err(TryRecvError::Empty) => drained = true,
                // 작업 스레드가 사라졌다(오류로 멈췄거나 만들지 못함): 읽는 중으로 두지 않고 안내를 보인다.
                Err(TryRecvError::Disconnected) => {
                    self.service = None;
                    self.rows.clear();
                    self.loaded_folder = None;
                    self.failure = Some(STOPPED_NOTICE.to_string());
                    changed = true;
                    drained = true;
                }
            }
        }
        changed
    }

    fn send(
        &mut self,
        force: bool,
    ) {
        self.failure = None;
        let request = Request {
            folder: self.watched.clone(),
            expanded: self.expanded_dirs.clone(),
            hidden: self.hidden.clone(),
            force,
        };
        if self.watched.is_some() || self.service.is_some() {
            let paused = self.paused;
            self.service
                .get_or_insert_with(|| {
                    let service = FileService::start();
                    service.set_paused(paused);
                    service
                })
                .request(request);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_files_21_showing_another_folder_keeps_the_open_folders_by_their_paths() {
        let _home = crate::test_support::TempHome::new();
        let mut files = FilesState::default();
        assert!(files.show(Some("/p".to_string()), true));
        files.toggle_dir("/p/src");
        assert!(!files.show(Some("/p".to_string()), true), "the same folder changes nothing");
        assert!(files.show(Some("/q".to_string()), true));
        assert_eq!(files.folder(), Some("/q"));
        assert!(files.rows().is_empty() && !files.is_loaded(), "the rows of the old folder are dropped");
        assert!(files.show(None, true));
        assert_eq!(files.folder(), None);
        assert!(files.show(Some("/p".to_string()), true));
        assert!(files.expanded_dirs.contains("/p/src"), "coming back finds the folder still open");
    }

    #[test]
    fn test_c_files_25_hiding_the_list_stops_reading_but_keeps_the_open_folders() {
        let _home = crate::test_support::TempHome::new();
        let mut files = FilesState::default();
        files.show(Some("/p".to_string()), true);
        files.toggle_dir("/p/src");
        assert!(files.show(Some("/p".to_string()), false), "hiding stops the reading");
        assert_eq!(files.folder(), Some("/p"));
        assert!(!files.is_loaded());
        assert!(files.show(Some("/p".to_string()), true));
        assert!(files.expanded_dirs.contains("/p/src"), "showing it again keeps the open folders");
    }

    #[test]
    fn test_c_files_26_an_added_entry_opens_its_parent_and_a_new_folder_opens_too() {
        let _home = crate::test_support::TempHome::new();
        let mut files = FilesState::default();
        files.show(Some("/p".to_string()), true);
        files.files_expanded = false;
        files.show_added("/p/src", Some("/p/src/sub"));
        assert!(files.expanded_dirs.contains("/p/src") && files.expanded_dirs.contains("/p/src/sub"));
        assert!(files.files_expanded, "the section opens so the new entry is visible");
        files.show_added("/p", None);
        assert!(files.expanded_dirs.contains("/p"));
        assert_eq!(files.expanded_dirs.len(), 3);
    }

    #[test]
    fn test_c_files_33_the_first_result_comes_from_another_thread_and_counts_the_folders_read() {
        let _home = crate::test_support::TempHome::new();
        let dir = tempfile::tempdir().unwrap();
        let mut files = FilesState::default();
        files.show(Some(dir.path().to_string_lossy().to_string()), true);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !files.is_loaded() && std::time::Instant::now() < deadline {
            files.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(files.is_loaded());
        assert!(files.read_off_window());
        assert_eq!(files.n_listings(), 1);
    }

    #[test]
    fn test_c_files_22_the_last_opened_folder_is_remembered_until_it_is_closed() {
        let _home = crate::test_support::TempHome::new();
        let mut files = FilesState::default();
        files.show(Some("/p".to_string()), true);
        files.toggle_dir("/p/src");
        assert_eq!(files.last_expanded.as_deref(), Some("/p/src"));
        files.toggle_dir("/p/lib");
        assert_eq!(files.last_expanded.as_deref(), Some("/p/lib"));
        files.toggle_dir("/p/src");
        assert_eq!(files.last_expanded.as_deref(), Some("/p/lib"), "closing another folder keeps it");
        files.toggle_dir("/p/lib");
        assert_eq!(files.last_expanded, None);
        assert!(files.expanded_dirs.is_empty());
    }

    #[test]
    fn test_a_files_thread_that_died_shows_a_notice_and_the_next_request_starts_a_new_one() {
        let _home = crate::test_support::TempHome::new();
        let dir = tempfile::tempdir().unwrap();
        let mut files = FilesState::default();
        files.show(Some(dir.path().to_string_lossy().to_string()), true);
        files.service = Some(FileService::stopped());
        assert!(files.poll(), "the loss is a visible change");
        assert_eq!(files.failure(), Some(STOPPED_NOTICE));
        assert!(!files.is_loaded() && files.rows().is_empty());
        assert!(!files.poll(), "the loss is reported once");
        files.refresh();
        assert_eq!(files.failure(), None, "a new request clears the notice");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !files.is_loaded() && std::time::Instant::now() < deadline {
            files.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(files.is_loaded(), "the new thread reads the folder");
    }

    #[test]
    fn test_c_d70_f2_goes_to_the_pty_when_the_list_has_no_focus() {
        // 행을 누르면 목록이 키를 받고, 키를 돌려주면(`blur_list`) 선택은 남되 받지 않는다.
        let _home = crate::test_support::TempHome::new();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        let mut files = FilesState::default();
        files.show(Some(folder.clone()), true);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !files.is_loaded() && std::time::Instant::now() < deadline {
            files.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let row = files.rows().first().map(|row| row.path.clone()).expect("a row");
        assert_eq!(files.focused_entry(), None, "no row is selected yet, so F2 and Ctrl+V go to the terminal");
        files.select_row(&row);
        assert_eq!(files.focused_entry(), Some((row.clone(), false)));
        files.focus_event(ListFocusEvent::KeysReturned);
        assert_eq!(files.focused_entry(), None, "the keys went back to the terminal");
        assert_eq!(files.selected_row(), Some(row.as_str()), "the selection is remembered");
        files.select_row("/not/a/row");
        assert_eq!(files.focused_entry(), None, "a row that is not in the list is not a target");
    }

    #[test]
    fn test_c_d70_renaming_a_folder_moves_the_selection_and_the_open_folders() {
        let _home = crate::test_support::TempHome::new();
        let mut files = FilesState::default();
        files.show(Some("/p".to_string()), true);
        files.toggle_dir("/p/src");
        files.toggle_dir("/p/src/deep");
        files.select_row("/p/src/deep");
        files.renamed("/p/src", "/p/lib");
        assert!(files.expanded_dirs.contains("/p/lib") && files.expanded_dirs.contains("/p/lib/deep"));
        assert_eq!(files.last_expanded.as_deref(), Some("/p/lib/deep"));
        assert_eq!(files.selected_row, Some("/p/lib/deep".to_string()));
    }
}
