//! `pen_save`: Pen에 열린 파일의 변경을 디스크로 저장한다.
//!
//! Pencil 도구는 저장 도구가 없어(Pen 1.2.21까지) 쓴 내용이 저장 전까지 앱 메모리에만 있다. 저장 경로는 호출마다 고른다: Pencil 도구 목록에
//! `save`가 있으면 그 도구로(공식 경로), 없으면 앱의 File > Save 메뉴를 접근성 기능으로 골라(메뉴 경로) 저장한다. 키 입력은 쓰지 않는다.
//! 공식 경로는 도구가 목록에 있는지만 확인했고 실제 앱에서 불러 본 적이 없다.
//!
//! 어느 경로든 파일을 활성 문서로 만든 뒤 저장하고, 디스크 수정 시각이 바뀌었는지로 저장을 판정한다. 저장 뒤에는 이 프로그램이 연 다른
//! `.pen` 파일의 수정 시각도 비교해, 저장이 다른 파일에 떨어지지 않았는지 확인한다.
//!
//! ```text
//! 저장 잠금 → 이전 활성 문서 기록 → 대상 창의 최소화 여부 기록
//!   → [시도마다] 활성 문서로 만들기(pen_open) → 활성 문서 재확인 → 대상·다른 연 파일 수정 시각 기록 → 저장(도구 또는 메뉴)
//!                → 대상 수정 시각 변화 대기 → 다른 연 파일 수정 시각 비교
//!   → 이전 활성 문서로 되돌리기(그 창이 최소화였으면 다시 최소화) ─(최소화였으면)→ 대상 창을 활성화 없이 다시 최소화 → 잠금 해제
//! ```
//! 최소화 여부는 활성 문서로 만들기 전에 읽는다. Pen은 대상을 활성 문서로 만들며 최소화된 창을 되살리므로, 그 뒤에 읽으면 최소화였던
//! 사실을 놓친다.
//! 이 순서는 `save_with`가 쥐고, 앱·창 조작은 `SaveSteps`의 구현(Windows의 `PenSave`)이 맡는다.
use std::time::{Instant, SystemTime};

use paddock_core::pen_paths::{PenTarget, normalize_document};

use crate::limits::Limits;
use crate::pencil::{PencilTool, PencilTools};
use crate::retry::{SaveError, with_retries};
#[cfg(windows)]
use crate::{
    menu,
    open::{open_without_turn, restore_active},
    pencil::{Pen, ToolOutcome},
    session::Session,
    titles::{Found, file_name_of},
    windows::{self, SaveLock, Window},
};

/// 저장을 부르는 경로.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveRoute {
    /// Pencil 저장 도구 `save{uri}`.
    Official,
    /// 문서 창의 File > Save 메뉴.
    Menu,
}

impl SaveRoute {
    /// 결과 글에 붙이는 경로 이름.
    fn label(self) -> &'static str {
        match self {
            Self::Official => "Pencil save tool",
            Self::Menu => "File menu",
        }
    }
}

/// Pencil 도구 구성으로 저장 경로를 고른다. 저장 도구가 있으면 공식 경로, 없으면(목록을 못 읽은 경우 포함) 메뉴 경로다.
///
/// Examples
/// --------
/// | 도구 목록                                  | 결과       |
/// |--------------------------------------------|------------|
/// | `execute`, `get_app_state` (Pen 1.2.14)    | `Menu`     |
/// | `execute`, `save`                          | `Official` |
/// | (못 읽음)                                  | `Menu`     |
pub fn choose_route(tools: &PencilTools) -> SaveRoute {
    if tools.has(PencilTool::SAVE) { SaveRoute::Official } else { SaveRoute::Menu }
}

/// 파일 수정 시각. 읽지 못하면 None이다.
fn modified_time(local_path: &str) -> Option<SystemTime> {
    std::fs::metadata(local_path).and_then(|metadata| metadata.modified()).ok()
}

/// 파일마다 지금의 수정 시각을 적어 둔다.
pub fn modified_times(targets: Vec<PenTarget>) -> Vec<(PenTarget, Option<SystemTime>)> {
    targets
        .into_iter()
        .map(|target| {
            let modified = modified_time(&target.local);
            (target, modified)
        })
        .collect()
}

/// 적어 둔 수정 시각과 지금 수정 시각이 다른 파일의 Windows 경로. 사라졌거나 새로 읽히게 된 파일도 달라진 것으로 본다.
pub fn changed_files(recorded: &[(PenTarget, Option<SystemTime>)]) -> Vec<String> {
    recorded
        .iter()
        .filter(|(target, before)| modified_time(&target.local) != *before)
        .map(|(target, _)| target.windows.clone())
        .collect()
}

/// 파일 수정 시각이 `before`와 달라지기를 `SAVE_DEADLINE`까지 기다린다.
fn wait_for_file_change(
    local_path: &str,
    before: Option<SystemTime>,
) -> bool {
    let deadline = Instant::now() + Limits::SAVE_DEADLINE;
    let mut changed = false;
    while !changed && Instant::now() < deadline {
        std::thread::sleep(Limits::SAVE_INTERVAL);
        changed = modified_time(local_path) != before;
    }
    changed
}

/// 활성 문서 표기가 대상인지 확인한다. 아니면 그 표기(열린 문서가 없으면 `none`)를 오류로 돌려준다.
///
/// Examples
/// --------
/// | 활성 문서         | 대상           | 결과                 |
/// |-------------------|----------------|----------------------|
/// | `/C:/w/a.pen`     | `C:\w\a.pen`   | `Ok`                 |
/// | `/C:/w/b.pen`     | `C:\w\a.pen`   | `Err("/C:/w/b.pen")` |
/// | (없음)            | `C:\w\a.pen`   | `Err("none")`        |
fn check_active(
    active: Option<String>,
    target: &PenTarget,
) -> Result<(), String> {
    if active.as_deref().map(normalize_document) == Some(normalize_document(&target.windows)) {
        Ok(())
    } else {
        Err(active.unwrap_or_else(|| "none".to_string()))
    }
}

/// 저장 한 번에 드는 앱 조작. 조작의 순서와 판정은 `save_with`가 정하고, 구현은 각 조작만 한다.
pub trait SaveSteps {
    /// 컴퓨터 전체에서 하나만 쥘 수 있는 저장 차례. 버리면 놓인다.
    type Turn;
    /// 대상의 편집 창을 가리키는 값.
    type Window: Copy;

    /// 저장 차례를 `SAVE_LOCK_DEADLINE`까지 기다려 잡는다. 시한을 넘기면 None이다.
    fn take_turn(&self) -> Option<Self::Turn>;
    /// 지금 활성 문서 표기. 열린 문서가 없거나 답을 못 받으면 None이다.
    fn active_document(&self) -> Option<String>;
    /// 대상의 편집 창이 지금 최소화돼 있으면 그 창. 창이 없거나, 이름이 같은 창이 여럿이거나, 최소화돼 있지 않으면 None이다.
    fn minimized_window(&self) -> Option<Self::Window>;
    /// 창을 다시 최소화하되 다른 창을 활성으로 바꾸지 않는다.
    fn minimize_without_activating(
        &self,
        window: Self::Window,
    );
    /// 대상을 활성 문서로 만든다. 못 하면 에이전트에게 보일 문장을 돌려준다.
    fn make_active(&self) -> Result<(), String>;
    /// 이 프로그램이 연 파일 가운데 대상이 아닌 것.
    fn opened_besides(&self) -> Vec<PenTarget>;
    /// 고른 경로로 저장을 부른다. `Ok`면 저장을 불렀다는 뜻이고, 디스크에 쓰였는지는 호출자가 수정 시각으로 확인한다.
    /// `Ok(Some(문장))`은 저장 도구가 오류를 냈거나 답하지 않은 사정으로, 저장이 확인되지 않으면 실패 문장에 붙인다.
    fn invoke_save(
        &self,
        route: SaveRoute,
    ) -> Result<Option<String>, SaveError>;
    /// 활성 문서를 `previous`로 되돌린다.
    fn restore_active(
        &self,
        previous: Option<&str>,
    );
}

/// 파일을 활성으로 만들고 저장을 부른 뒤 수정 시각 변화를 확인한다. 한 번의 시도이고, 저장 잠금을 쥔 채로 불린다.
fn save_once<S: SaveSteps>(
    steps: &S,
    route: SaveRoute,
    target: &PenTarget,
) -> Result<String, SaveError> {
    steps.make_active().map_err(SaveError::Fatal)?;
    check_active(steps.active_document(), target).map_err(|active| {
        SaveError::Retryable(format!(
            "The active document changed ({active}) just before saving, so Save was not chosen: {}",
            target.windows
        ))
    })?;
    // 저장 판정의 기준은 저장을 부르기 바로 전에 읽는다. 더 일찍 읽으면 그 사이 다른 저장이 바꾼 수정 시각을 이번 저장으로 오인한다.
    let before = std::fs::metadata(&target.local).map_err(|error| SaveError::Fatal(error.to_string()))?;
    let others = modified_times(steps.opened_besides());
    let note = steps.invoke_save(route)?;
    let is_saved = wait_for_file_change(&target.local, before.modified().ok());
    let changed_others = changed_files(&others);
    if !changed_others.is_empty() {
        Err(SaveError::Fatal(format!(
            "Saving {} also changed another file this tool opened: {}. The save may have gone to the wrong document, so check those files; it was not tried again.",
            target.windows,
            changed_others.join(", ")
        )))
    } else if is_saved {
        let after = std::fs::metadata(&target.local).map_or(0, |metadata| metadata.len());
        Ok(format!("Saved: {} ({} -> {after} bytes, {})", target.windows, before.len(), route.label()))
    } else {
        Err(SaveError::Fatal(format!(
            "{}The file's modified time did not change. It was not saved: {}",
            note.map(|note| format!("{note} ")).unwrap_or_default(),
            target.windows
        )))
    }
}

/// 저장 잠금을 쥔 채로 대상을 저장하고 이전 활성 문서로 되돌린다.
///
/// 왜: 메뉴의 Save는 앱의 활성 문서를 저장하므로, 활성 문서를 바꾸는 일(대상 열기·되돌리기)과 메뉴 조작이 모두 한 잠금 안에 있어야 다른
/// 프로세스의 열기·저장이 그 사이 활성 문서를 바꾸지 못한다. 이전 활성 문서도 잠금을 잡은 뒤 읽는다(앞 저장자가 잠시 바꾼 문서를 되돌릴
/// 곳으로 삼지 않는다).
///
/// 대상 창이 최소화돼 있었으면 이전 활성 문서로 되돌린 뒤 활성화 없이 다시 최소화한다. 최소화 여부는 대상을 활성 문서로 만들기 전에
/// 읽는다: Pen이 대상을 활성 문서로 만들며 창을 되살리므로 그 뒤에는 최소화였는지 알 수 없다. 저장이 실패해도 다시 최소화한다.
///
/// 저장을 부르기 전의 일시적 실패(창·메뉴를 못 찾음, 활성 문서가 바뀜)만 `SAVE_ATTEMPTS`번까지 다시 시도하고, 저장을 부른 뒤에는 다시
/// 시도하지 않는다. 잠금을 시한 안에 못 잡으면 아무것도 하지 않고 실패한다.
///
/// Examples
/// --------
/// | 상황                                   | 결과                                                   |
/// |----------------------------------------|--------------------------------------------------------|
/// | 잠금 시한 넘김                         | `Err`, 활성 문서를 건드리지 않음                       |
/// | 첫 시도에서 활성 문서가 바뀜, 둘째 성공 | `Ok("Saved: …")`, 저장은 한 번만 부름                  |
/// | 저장 뒤 다른 연 파일의 수정 시각이 바뀜 | `Err`(그 파일 이름), 다시 시도하지 않음                |
/// | 대상 창이 최소화돼 있었음              | 결과는 위와 같고, 되돌린 뒤 대상 창이 다시 최소화됨    |
fn save_with<S: SaveSteps>(
    steps: &S,
    route: SaveRoute,
    target: &PenTarget,
) -> Result<String, String> {
    match steps.take_turn() {
        None => Err(format!(
            "Another pen_open or pen_save is still running, so Save was not chosen. Call again: {}",
            target.windows
        )),
        Some(turn) => {
            let previous = steps.active_document();
            let minimized = steps.minimized_window();
            let result =
                with_retries(Limits::SAVE_ATTEMPTS, Limits::SAVE_RETRY_DELAY, || save_once(steps, route, target));
            steps.restore_active(previous.as_deref());
            // 이전 활성 문서로 되돌린 뒤에 최소화한다. 대상이 아직 활성 창일 때 최소화하면 앞 창이 바뀌는 사이 활성 문서가 흔들린다.
            if let Some(window) = minimized {
                steps.minimize_without_activating(window);
            }
            drop(turn);
            result
        }
    }
}

/// Pen 앱에서 한 파일을 저장하는 실제 조작.
#[cfg(windows)]
struct PenSave<'a> {
    session: &'a Session,
    pen: &'a Pen,
    input: &'a str,
    target: &'a PenTarget,
}

#[cfg(windows)]
impl SaveSteps for PenSave<'_> {
    type Turn = SaveLock;
    type Window = Window;

    fn take_turn(&self) -> Option<SaveLock> {
        SaveLock::acquire(Limits::SAVE_LOCK_DEADLINE)
    }

    fn active_document(&self) -> Option<String> {
        self.pen.active_document()
    }

    /// 창이 나타나기를 기다리지 않는다: 아직 열리지 않은 파일은 되살릴 최소화 창도 없다.
    fn minimized_window(&self) -> Option<Window> {
        match windows::find_editor_window(file_name_of(&self.target.windows)) {
            Found::One(window) if windows::is_minimized(window) => Some(window),
            Found::None | Found::One(_) | Found::Ambiguous => None,
        }
    }

    fn minimize_without_activating(
        &self,
        window: Window,
    ) {
        windows::minimize_without_activating(window);
    }

    /// 앱이 빈 문서로 읽은 파일은 여기서 거부된다(`pen_open`과 같은 판정).
    fn make_active(&self) -> Result<(), String> {
        open_without_turn(self.session, Some(self.input)).map(|_| ())
    }

    fn opened_besides(&self) -> Vec<PenTarget> {
        self.session.opened_besides(self.target)
    }

    fn invoke_save(
        &self,
        route: SaveRoute,
    ) -> Result<Option<String>, SaveError> {
        match route {
            // 도구가 오류를 내거나 답하지 않아도 파일은 쓰였을 수 있어, 판정은 수정 시각에 맡기고 도구의 말은 사정으로만 남긴다.
            SaveRoute::Official => Ok(match self.pen.save_by_tool(&self.target.windows) {
                ToolOutcome::Succeeded => None,
                ToolOutcome::Failed(reason) => Some(format!("The Pencil save tool reported an error ({reason}).")),
                ToolOutcome::NoAnswer => Some(format!(
                    "The Pencil save tool gave no answer within {} seconds.",
                    Limits::PENCIL_SAVE_DEADLINE.as_secs()
                )),
            }),
            SaveRoute::Menu => {
                let file_name = file_name_of(&self.target.windows);
                menu::save_by_menu(file_name, || check_active(self.pen.active_document(), self.target))
                    .map(|()| None)
                    .map_err(|failure| failure.into_save_error(file_name))
            }
        }
    }

    fn restore_active(
        &self,
        previous: Option<&str>,
    ) {
        restore_active(self.pen, previous, &self.target.windows);
    }
}

/// `pen_save`: Pen에 열린 파일을 디스크에 저장한다. 메모리의 변경을 사람 없이 파일로 남기는 유일한 경로다.
///
/// 파일이 활성이 아니면 잠시 활성으로 바꾸고(앱이 빈 문서로 읽은 파일은 열기 단계에서 거부), 저장 뒤 이전 활성 문서로 되돌린다.
/// 파일의 창이 최소화돼 있었으면 되돌린 뒤 다시 최소화한다.
/// 저장을 부른 뒤 수정 시각이 바뀌지 않거나, 이 프로그램이 연 다른 파일의 수정 시각이 바뀌면 실패다. 변경이 없어도 앱이 파일을 다시 써서
/// 수정 시각은 바뀐다(Pen 1.2.14 실측). 저장을 부르기 전의 일시적 실패(창·메뉴를 못 찾음, 활성 문서가 바뀜)만 `SAVE_ATTEMPTS`번까지
/// 다시 시도하고, 저장을 부른 뒤에는 다시 시도하지 않는다. 다른 열기·저장이 `SAVE_LOCK_DEADLINE` 넘게 이어지면 아무것도 하지 않고 실패한다.
#[cfg(windows)]
pub fn pen_save(
    session: &Session,
    input: Option<&str>,
) -> Result<String, String> {
    let target = session.resolve(input)?;
    let input = input.unwrap_or_default();
    let pen = Pen::find()?;
    let route = choose_route(&pen.tools());
    save_with(&PenSave { session, pen: &pen, input, target: &target }, route, &target)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    #[test]
    fn test_save_1_the_route_is_official_only_when_pencil_lists_a_save_tool() {
        let old = PencilTools::from_list(&json!({ "tools": [
            { "name": "execute", "inputSchema": { "required": ["filePath", "input"] } },
            { "name": "get_app_state" }
        ] }));
        assert_eq!(choose_route(&old), SaveRoute::Menu);
        let new = PencilTools::from_list(&json!({ "tools": [
            { "name": "execute", "inputSchema": { "required": ["uri", "input"] } },
            { "name": "save", "inputSchema": { "required": ["uri"] } }
        ] }));
        assert_eq!(choose_route(&new), SaveRoute::Official);
        assert_eq!(choose_route(&PencilTools::default()), SaveRoute::Menu);
    }

    /// 임시 폴더에 내용 `x`인 파일을 만들고 그 대상을 돌려준다.
    fn file_target(
        directory: &tempfile::TempDir,
        name: &str,
    ) -> PenTarget {
        let path = directory.path().join(name);
        std::fs::write(&path, "x").unwrap();
        let path = path.to_string_lossy().to_string();
        PenTarget { local: path.clone(), windows: path }
    }

    /// 파일 수정 시각을 지금보다 `seconds_later`초 뒤로 옮긴다(다른 프로세스가 그 파일을 쓴 것처럼).
    fn touch(
        target: &PenTarget,
        seconds_later: u64,
    ) {
        let later = SystemTime::now() + Duration::from_secs(seconds_later);
        std::fs::File::options().write(true).open(&target.local).unwrap().set_modified(later).unwrap();
    }

    #[test]
    fn test_save_2_only_files_whose_modified_time_moved_are_reported() {
        let directory = tempfile::tempdir().unwrap();
        let (kept, touched, removed) = (
            file_target(&directory, "kept.pen"),
            file_target(&directory, "touched.pen"),
            file_target(&directory, "removed.pen"),
        );
        let recorded = modified_times(vec![kept.clone(), touched.clone(), removed.clone()]);
        assert!(changed_files(&recorded).is_empty());
        touch(&touched, 5);
        std::fs::remove_file(&removed.local).unwrap();
        assert_eq!(changed_files(&recorded), vec![touched.windows, removed.windows]);
    }

    #[test]
    fn test_save_3_the_active_document_check_compares_normalized_spellings() {
        let target = PenTarget { local: "/w/a.pen".into(), windows: "C:\\w\\a.pen".into() };
        assert_eq!(check_active(Some("/C:/w/a.pen".into()), &target), Ok(()));
        assert_eq!(check_active(Some("/C:/w/b.pen".into()), &target), Err("/C:/w/b.pen".into()));
        assert_eq!(check_active(None, &target), Err("none".into()));
    }

    /// 조작마다 이름을 남기는 가짜 앱. 활성 문서는 `actives`의 앞에서부터 하나씩 답하고(바닥나면 마지막 값), 파일 변화는 닫힌 함수로 흉내 낸다.
    struct FakeApp {
        log: Rc<RefCell<Vec<&'static str>>>,
        is_turn_free: bool,
        /// 대상 창이 처음에 최소화돼 있었는지.
        is_minimized: bool,
        actives: RefCell<Vec<Option<String>>>,
        others: Vec<PenTarget>,
        /// 차례를 잡을 때 할 일(잠금을 기다리는 동안 다른 저장이 파일을 쓴 상황).
        on_turn: Box<dyn Fn()>,
        /// 저장을 부를 때 할 일(앱이 파일을 쓴 상황).
        on_save: Box<dyn Fn()>,
        n_saves: Cell<usize>,
    }

    /// 버릴 때 `release`를 남기는 가짜 저장 차례.
    struct FakeTurn(Rc<RefCell<Vec<&'static str>>>);

    impl Drop for FakeTurn {
        fn drop(&mut self) {
            self.0.borrow_mut().push("release");
        }
    }

    impl FakeApp {
        fn new(actives: &[&str]) -> Self {
            Self {
                log: Rc::new(RefCell::new(Vec::new())),
                is_turn_free: true,
                is_minimized: false,
                actives: RefCell::new(actives.iter().rev().map(|active| Some(active.to_string())).collect()),
                others: Vec::new(),
                on_turn: Box::new(|| {}),
                on_save: Box::new(|| {}),
                n_saves: Cell::new(0),
            }
        }

        fn note(
            &self,
            step: &'static str,
        ) {
            self.log.borrow_mut().push(step);
        }

        fn steps(&self) -> Vec<&'static str> {
            self.log.borrow().clone()
        }
    }

    impl SaveSteps for FakeApp {
        type Turn = FakeTurn;
        type Window = ();

        fn take_turn(&self) -> Option<FakeTurn> {
            self.note("take_turn");
            (self.on_turn)();
            self.is_turn_free.then(|| FakeTurn(Rc::clone(&self.log)))
        }

        fn active_document(&self) -> Option<String> {
            self.note("active_document");
            let mut actives = self.actives.borrow_mut();
            if actives.len() > 1 { actives.pop().flatten() } else { actives.last().cloned().flatten() }
        }

        /// 실제 앱처럼 대상을 활성으로 만든 뒤에는 최소화가 풀리므로, 그 뒤에 물으면 최소화가 아니라고 답한다.
        fn minimized_window(&self) -> Option<()> {
            self.note("minimized_window");
            let is_restored = self.log.borrow().contains(&"make_active");
            (self.is_minimized && !is_restored).then_some(())
        }

        fn minimize_without_activating(
            &self,
            _window: (),
        ) {
            self.note("minimize");
        }

        fn make_active(&self) -> Result<(), String> {
            self.note("make_active");
            Ok(())
        }

        fn opened_besides(&self) -> Vec<PenTarget> {
            self.note("opened_besides");
            self.others.clone()
        }

        fn invoke_save(
            &self,
            _route: SaveRoute,
        ) -> Result<Option<String>, SaveError> {
            self.note("invoke_save");
            self.n_saves.set(self.n_saves.get() + 1);
            (self.on_save)();
            Ok(None)
        }

        fn restore_active(
            &self,
            _previous: Option<&str>,
        ) {
            self.note("restore_active");
        }
    }

    #[test]
    fn test_save_4_everything_from_reading_the_previous_document_to_restoring_it_happens_inside_the_turn() {
        let directory = tempfile::tempdir().unwrap();
        let target = file_target(&directory, "a.pen");
        let saved = target.clone();
        let app =
            FakeApp { on_save: Box::new(move || touch(&saved, 5)), ..FakeApp::new(&["/C:/w/c.pen", &target.windows]) };
        let result = save_with(&app, SaveRoute::Menu, &target);
        assert!(result.as_deref().is_ok_and(|text| text.starts_with("Saved: ")), "{result:?}");
        assert_eq!(
            app.steps(),
            [
                "take_turn",
                "active_document",
                "minimized_window",
                "make_active",
                "active_document",
                "opened_besides",
                "invoke_save",
                "restore_active",
                "release"
            ]
        );
    }

    #[test]
    fn test_save_5_a_changed_active_document_is_retried_without_saving_and_save_is_invoked_once() {
        let directory = tempfile::tempdir().unwrap();
        let target = file_target(&directory, "a.pen");
        let saved = target.clone();
        let app = FakeApp {
            on_save: Box::new(move || touch(&saved, 5)),
            ..FakeApp::new(&["/C:/w/c.pen", "/C:/w/b.pen", &target.windows])
        };
        let result = save_with(&app, SaveRoute::Menu, &target);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(app.n_saves.get(), 1);
        let steps = app.steps();
        let n_opens = steps.iter().filter(|step| **step == "make_active").count();
        let n_opens_before_save =
            steps.iter().take_while(|step| **step != "invoke_save").filter(|step| **step == "make_active").count();
        assert_eq!((n_opens, n_opens_before_save), (2, 2));
        assert_eq!(steps.first(), Some(&"take_turn"));
        assert_eq!(steps.last(), Some(&"release"));
    }

    #[test]
    fn test_save_6_no_turn_means_nothing_is_touched() {
        let directory = tempfile::tempdir().unwrap();
        let target = file_target(&directory, "a.pen");
        let app = FakeApp { is_turn_free: false, ..FakeApp::new(&[&target.windows]) };
        let result = save_with(&app, SaveRoute::Menu, &target);
        assert!(result.is_err_and(|message| message.contains("still running")));
        assert_eq!(app.steps(), ["take_turn"]);
    }

    #[test]
    fn test_save_7_another_opened_file_that_changed_fails_the_save_without_a_retry() {
        let directory = tempfile::tempdir().unwrap();
        let (target, other) = (file_target(&directory, "a.pen"), file_target(&directory, "b.pen"));
        let (saved, wrong) = (target.clone(), other.clone());
        let app = FakeApp {
            others: vec![other.clone()],
            on_save: Box::new(move || {
                touch(&saved, 5);
                touch(&wrong, 5);
            }),
            ..FakeApp::new(&[&target.windows])
        };
        let result = save_with(&app, SaveRoute::Menu, &target);
        assert!(result.is_err_and(|message| message.contains(&other.windows)));
        assert_eq!(app.n_saves.get(), 1);
    }

    #[test]
    fn test_save_8_a_write_by_another_saver_while_waiting_for_the_turn_does_not_count_as_this_save() {
        // 잠금을 기다리는 동안 다른 저장이 대상을 썼고, 이번 저장은 아무것도 쓰지 않았다. 판정 기준을 잠금 뒤에 읽어야 실패로 판정된다.
        let directory = tempfile::tempdir().unwrap();
        let target = file_target(&directory, "a.pen");
        let written = target.clone();
        let app = FakeApp { on_turn: Box::new(move || touch(&written, 5)), ..FakeApp::new(&[&target.windows]) };
        let result = save_with(&app, SaveRoute::Menu, &target);
        assert!(result.is_err_and(|message| message.contains("did not change")));
        assert_eq!(app.n_saves.get(), 1);
    }

    #[test]
    fn test_save_9_a_minimized_target_is_recorded_before_it_is_made_active_and_minimized_again_after_restoring() {
        // 대상을 활성으로 만들면 최소화가 풀린다. 최소화 여부를 그 전에 읽어야 끝에 다시 최소화한다.
        let directory = tempfile::tempdir().unwrap();
        let target = file_target(&directory, "a.pen");
        let saved = target.clone();
        let app = FakeApp {
            is_minimized: true,
            on_save: Box::new(move || touch(&saved, 5)),
            ..FakeApp::new(&["/C:/w/b.pen", &target.windows])
        };
        let result = save_with(&app, SaveRoute::Menu, &target);
        assert!(result.is_ok(), "{result:?}");
        let steps = app.steps();
        assert_eq!(steps[..4], ["take_turn", "active_document", "minimized_window", "make_active"]);
        assert_eq!(steps[steps.len() - 3..], ["restore_active", "minimize", "release"]);
    }

    #[test]
    fn test_save_10_a_minimized_target_is_minimized_again_even_when_the_save_fails() {
        let directory = tempfile::tempdir().unwrap();
        let target = file_target(&directory, "a.pen");
        let app = FakeApp { is_minimized: true, ..FakeApp::new(&["/C:/w/b.pen", &target.windows]) };
        let result = save_with(&app, SaveRoute::Menu, &target);
        assert!(result.is_err_and(|message| message.contains("did not change")));
        let steps = app.steps();
        assert_eq!(steps[steps.len() - 3..], ["restore_active", "minimize", "release"]);
    }

    #[test]
    fn test_save_11_a_target_that_was_not_minimized_is_not_minimized() {
        let directory = tempfile::tempdir().unwrap();
        let target = file_target(&directory, "a.pen");
        let saved = target.clone();
        let app =
            FakeApp { on_save: Box::new(move || touch(&saved, 5)), ..FakeApp::new(&["/C:/w/b.pen", &target.windows]) };
        let result = save_with(&app, SaveRoute::Menu, &target);
        assert!(result.is_ok(), "{result:?}");
        assert!(!app.steps().contains(&"minimize"));
    }
}
