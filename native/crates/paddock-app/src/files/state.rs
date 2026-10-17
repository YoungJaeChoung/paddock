//! 파일 목록의 창 쪽 상태: 지금 보여 주는 폴더, 펼친 폴더, 마지막 읽기 결과(행). 읽기는 `FileService`가 맡는다.
use std::collections::HashSet;

use paddock_core::file_tree::{FileRow, creation_folder, hidden_names, rows};

use super::{FileService, Request};
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
        while let Some(snapshot) = self.service.as_ref().and_then(FileService::try_recv) {
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
        changed
    }

    fn send(
        &mut self,
        force: bool,
    ) {
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
}
