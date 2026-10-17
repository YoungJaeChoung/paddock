//! 앱이 남기는 `~/.paddock/native/state.json`: 열려 있던 터미널의 id·이름·순서와 선택한 터미널.
//!
//! 다시 실행할 때 데몬에 아직 있는 터미널만 되살린다. 파일이 없거나 깨져도 앱이 시작되도록 읽기는 실패하지 않는다.
use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use paddock_core::session_state::SessionState;
use paddock_protocol::daemon_info::{ensure_private_dir, native_dir, write_private_file};

fn state_path() -> PathBuf {
    native_dir().join("state.json")
}

/// 저장된 상태에서 `live_ids`(지금 `daemon`에 있는 터미널)에 있는 터미널만 읽는다.
/// 파일이 없거나 깨졌거나 다른 데몬이 저장한 것이면 빈 상태다.
pub fn load(
    live_ids: &HashSet<u32>,
    daemon: &str,
) -> SessionState {
    std::fs::read_to_string(state_path()).map(|text| SessionState::restore(&text, live_ids, daemon)).unwrap_or_default()
}

/// 저장된 상태를 터미널이 지금 있는지 가리지 않고 저장된 그대로 읽는다(`daemon`은 저장한 데몬의 식별 글자).
/// 데몬이 바뀐 뒤 옛 터미널이 있던 자리를 알아내 되살리는 데 쓴다. 파일이 없거나 깨졌으면 빈 상태다.
pub fn load_saved() -> SessionState {
    std::fs::read_to_string(state_path()).map(|text| SessionState::restore_saved(&text)).unwrap_or_default()
}

/// 상태를 저장한다. 소유자만 읽는 임시 파일(`state.json.tmp`)에 다 쓴 뒤 바꿔치워, 쓰는 도중 앱이 죽어도 이전 파일이 남는다.
pub fn save(state: &SessionState) -> io::Result<()> {
    ensure_private_dir(&native_dir())?;
    write_private_file(&state_path(), state.serialize().as_bytes())
}

#[cfg(test)]
mod tests {
    use paddock_core::file_kind::FileKind;
    use paddock_core::file_pane::FilePanes;
    use paddock_core::layout_model::Layout;
    use paddock_core::work_model::{GroupKey, WorkState};

    use super::*;
    use crate::test_support::TempHome;

    const DAEMON: &str = "10@t0";

    fn live(ids: &[u32]) -> HashSet<u32> {
        ids.iter().copied().collect()
    }

    #[test]
    fn test_missing_file_is_an_empty_state() {
        let _home = TempHome::new();
        assert_eq!(load(&live(&[1, 2]), DAEMON), SessionState::default());
    }

    #[test]
    fn test_saved_state_comes_back_with_membership_and_selection_and_drops_missing_ids() {
        let _home = TempHome::new();
        let mut work = WorkState::default();
        work.assign_terminal(5, "/home/me/app", "terminal 2");
        work.add_group(2, "한글");
        let mut files = FilePanes::default();
        files.open(&GroupKey::Folder("/home/me/app".into()), "/home/me/app/a.rs", FileKind::Text);
        files.open(&GroupKey::Group(2), "/tmp/한글.txt", FileKind::Text);
        let state = SessionState {
            daemon: DAEMON.into(),
            work,
            selected: Some(2),
            layouts: vec![Layout::pane(vec![5, 2], 1)],
            sidebar_collapsed: true,
            files,
            files_collapsed: true,
        };
        save(&state).unwrap();
        assert_eq!(load(&live(&[2, 5]), DAEMON), state);
        let partial = load(&live(&[5]), DAEMON);
        assert_eq!(partial.terminal_ids(), vec![5]);
        assert_eq!(partial.layouts, vec![Layout::pane(vec![5], 0)]);
        assert_eq!(partial.selected, None);
        assert_eq!(partial.files.len(), 1, "the folder keeps its file tab, the vanished group does not");
    }

    #[test]
    fn test_broken_file_starts_empty_and_save_replaces_it() {
        let _home = TempHome::new();
        ensure_private_dir(&native_dir()).unwrap();
        std::fs::write(state_path(), "{oops").unwrap();
        assert_eq!(load(&live(&[1]), DAEMON), SessionState::default());
        save(&SessionState::default()).unwrap();
        assert_eq!(load(&live(&[1]), DAEMON), SessionState::default());
        assert!(!state_path().with_extension("json.tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_saved_state_is_readable_by_the_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let _home = TempHome::new();
        save(&SessionState::default()).unwrap();
        assert_eq!(std::fs::metadata(state_path()).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
