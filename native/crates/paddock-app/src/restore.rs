//! 터미널 되살리기(앱 쪽): 데몬이 바뀐 뒤 앱을 다시 열면, 옛 데몬이 남긴 터미널 기록으로 같은 폴더·같은 이름·같은 대화의 터미널을 다시 연다.
//!
//! 기록을 쓰는 쪽은 데몬이다(`~/.paddock/native/terminal-history/<데몬 식별>/<터미널 번호>.json`). 이 모듈은 state.json이 가리키는
//! 옛 데몬의 폴더를 읽어 터미널마다 새 터미널을 열고, 이전 화면과 안내 줄을 첫 화면으로 넣고, 프롬프트가 뜨면 이어하기 명령을 입력한다.
//! 열지 못한 터미널은 건너뛰고 나머지를 계속 연다. 되살린 뒤에는 옛 데몬의 기록 폴더를 지운다.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use paddock_core::account_launch::Launch;
use paddock_core::agent_account::Runtime;
use paddock_core::agent_model::is_shell;
use paddock_core::session_state::SessionState;
use paddock_core::shell_catalog::{default_shell_name, wsl_folder};
use paddock_core::terminal_history::{HistoryRecord, parse_record};
use paddock_core::terminal_restore::{
    PromptCheck, RestorePlan, RestoreTarget, plan_restore, prompt_check, restored_screen,
};
use paddock_protocol::shared_config::history_dir;

use crate::accounts::launch::account_want;
use crate::accounts::store::registered_profiles;
use crate::app_log;
use crate::daemon_client::DaemonConnection;
use crate::settings::Settings;
use crate::state_store;
use crate::terminal::grid::Grid;
use crate::terminal::session::{TerminalSession, Want, start_session_with_screen};

/// 프롬프트가 뜨기를 확인하는 간격.
const PROMPT_POLL: Duration = Duration::from_millis(100);

/// 옛 데몬이 남긴 되살릴 터미널들.
pub struct Revival {
    /// 저장된 그대로의 옛 상태. 터미널이 있던 폴더·묶음·이름·배치를 알려 준다.
    pub saved: SessionState,
    /// 옛 터미널 번호 → 기록.
    pub records: BTreeMap<u32, HistoryRecord>,
    folder: PathBuf,
}

/// 되살린 결과.
pub struct Revived {
    /// 저장된 상태에 있던 터미널의 옛 번호 → 새 터미널.
    pub known: BTreeMap<u32, TerminalSession>,
    /// 저장된 상태에 없던 기록의 새 터미널과 그 이름(계정 터미널이면 계정 이름, 아니면 None). 한 묶음으로 모인다.
    pub unknown: Vec<(TerminalSession, Option<String>)>,
    /// 새 터미널로 다시 연 기록의 옛 터미널 번호. 이 기록만 지운다.
    pub restored: BTreeSet<u32>,
}

/// state.json이 가리키는 데몬이 지금 데몬(`daemon_id`)과 다르면 그 데몬이 남긴 기록을 읽는다. 같은 데몬이거나 기록 폴더가 없으면 None이다.
pub fn collect(daemon_id: &str) -> Option<Revival> {
    let saved = state_store::load_saved();
    let folder = history_dir(&saved.daemon);
    (!saved.daemon.is_empty() && saved.daemon != daemon_id && folder.is_dir()).then(|| Revival {
        records: read_records(&folder),
        saved,
        folder,
    })
}

/// 폴더의 `<번호>.json` 기록들. 읽을 수 없거나 모르는 형식의 파일은 건너뛴다.
fn read_records(folder: &Path) -> BTreeMap<u32, HistoryRecord> {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let id: u32 = path.file_stem()?.to_str()?.parse().ok()?;
            let record = parse_record(&std::fs::read_to_string(&path).ok()?)?;
            (path.extension()? == "json").then_some((id, record))
        })
        .collect()
}

/// 기록의 계획을 데몬에 보낼 터미널 요청으로 바꾼다.
///
/// 계정 터미널은 계정 터미널 요청을 다시 만들고(대화가 있으면 이어 열도록), 계정이 지워졌으면 기록된 실행 명세로 연다.
/// 시작 폴더가 사라졌으면 두 경우 모두 시작 폴더 없이 연다. 일반 터미널은 폴더가 사라졌으면 시작 폴더 없이 연다.
pub fn want_of(plan: &RestorePlan) -> Want {
    match &plan.target {
        RestoreTarget::Shell { shell, wsl_distribution, cwd } => {
            let usable = cwd.clone().filter(|folder| folder_is_usable(folder, shell == "wsl"));
            Want::New { shell: shell.clone(), cwd: usable, wsl_distribution: wsl_distribution.clone() }
        }
        RestoreTarget::Account { launch, resume_session_id } => {
            let is_wsl = launch.account.as_ref().is_some_and(|account| account.runtime == Runtime::Wsl);
            let usable = launch.cwd.clone().filter(|folder| folder_is_usable(folder, is_wsl));
            let current = launch.account.as_ref().map(|opened| {
                registered_profiles()
                    .into_iter()
                    .find(|profile| profile.id == opened.id)
                    .unwrap_or_else(|| opened.clone())
            });
            let home = std::env::home_dir().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_default();
            let rebuilt = current.as_ref().and_then(|profile| {
                account_want(
                    profile,
                    usable.as_deref(),
                    &home,
                    Settings::load().agent_permissions(),
                    cfg!(windows),
                    resume_session_id.as_deref(),
                )
                .ok()
            });
            rebuilt.unwrap_or_else(|| {
                let launch = Launch { cwd: usable, ..launch.clone() };
                Want::Launch {
                    shell: fallback_shell(&launch).to_string(),
                    wsl_distribution: None,
                    launch: Box::new(launch),
                }
            })
        }
    }
}

/// 이 컴퓨터에서 시작 폴더로 쓸 수 있는 폴더인가. WSL 안의 경로는 WSL이 확인하므로 그대로 쓴다.
fn folder_is_usable(
    folder: &str,
    is_wsl: bool,
) -> bool {
    is_wsl || wsl_folder(folder).is_some() || Path::new(folder).is_dir()
}

/// 기록된 실행 명세로 열 때 쓰는 셸 이름: 계정의 실제 실행 환경(WSL 계정은 `wsl`, 네이티브 계정은 이 운영체제의 기본 셸)이다.
pub fn fallback_shell(launch: &Launch) -> &'static str {
    let is_wsl = launch
        .account
        .as_ref()
        .map_or_else(|| launch.program.to_lowercase().contains("wsl"), |account| account.runtime == Runtime::Wsl);
    if is_wsl { "wsl" } else { default_shell_name(cfg!(windows), false) }
}

/// 저장된 상태에 없던 기록의 터미널 이름: 계정 터미널이면 정상으로 열 때와 같은 계정 이름, 아니면 `fallback`이 만든 기본 이름.
pub fn revived_name(
    title: Option<&str>,
    fallback: impl FnOnce() -> String,
) -> String {
    title.filter(|title| !title.is_empty()).map_or_else(fallback, str::to_string)
}

/// 되살릴 터미널을 모두 연다. 저장된 상태의 순서대로 연 뒤 상태에 없던 기록을 연다. 열지 못한 터미널은 기록에 남기고 건너뛴다.
///
/// `connect`는 터미널마다 데몬 연결을 하나씩 건넨다.
pub fn start_all(
    revival: &Revival,
    mut connect: impl FnMut() -> std::io::Result<DaemonConnection>,
    started: Instant,
) -> Revived {
    let in_saved = revival.saved.terminal_ids();
    let order: Vec<u32> = in_saved
        .iter()
        .copied()
        .chain(revival.records.keys().copied().filter(|id| !in_saved.contains(id)))
        .filter(|id| revival.records.contains_key(id))
        .collect();
    let mut revived = Revived { known: BTreeMap::new(), unknown: Vec::new(), restored: BTreeSet::new() };
    for old_id in order {
        let record = &revival.records[&old_id];
        let plan = plan_restore(record);
        let screen = restored_screen(&record.output, &plan.notice);
        let want = want_of(&plan);
        let title = want.title().map(str::to_string);
        match connect().and_then(|connection| start_session_with_screen(connection, want, Some(screen), started)) {
            Ok(session) => {
                revived.restored.insert(old_id);
                if let Some(command) = plan.command.clone() {
                    type_when_ready(session.grid.clone(), command);
                }
                if in_saved.contains(&old_id) {
                    revived.known.insert(old_id, session);
                } else {
                    revived.unknown.push((session, title));
                }
            }
            // 오류 문장에는 폴더 경로가 들어 있을 수 있어 로그에는 종류만 남긴다.
            Err(error) => app_log::event(&format!("could not restore terminal {old_id}: {:?}", error.kind())),
        }
    }
    revived
}

/// 새 셸이 프롬프트를 내면 이어하기 명령을 입력하는 스레드를 띄운다. 시간 안에 준비되지 않거나 셸 대신 다른 프로그램이 앞에 있으면 입력하지 않는다.
fn type_when_ready(
    grid: Arc<Grid>,
    command: String,
) {
    std::thread::spawn(move || {
        let waiting_since = Instant::now();
        let outcome = loop {
            let quiet = grid
                .shared
                .last_output_at
                .lock()
                .unwrap()
                .map(|at| u64::try_from(at.elapsed().as_millis()).unwrap_or(u64::MAX));
            let program = grid.shared.state.lock().unwrap().as_ref().and_then(|state| state.program.clone());
            let foreground_is_shell = program.as_deref().map(|name| is_shell(Some(name)));
            let elapsed = u64::try_from(waiting_since.elapsed().as_millis()).unwrap_or(u64::MAX);
            let check = prompt_check(elapsed, quiet, foreground_is_shell);
            if check != PromptCheck::Waiting {
                break check;
            }
            std::thread::sleep(PROMPT_POLL);
        };
        if outcome == PromptCheck::Ready {
            grid.input(format!("{command}\r").into_bytes());
        } else {
            app_log::event(&format!("restore: the resume command was not typed ({outcome:?})"));
        }
    });
}

/// 되살리기를 마쳤다: 새 터미널로 다시 연 기록(`restored`)만 지운다. 열지 못한 기록은 남기고, 비게 된 폴더는 지운다.
pub fn finish(
    revival: Revival,
    restored: &BTreeSet<u32>,
) {
    for id in restored {
        let _ = std::fs::remove_file(revival.folder.join(format!("{id}.json")));
    }
    // 폴더가 비었을 때만 지워진다. 남은 기록이 있으면 실패하고 폴더는 그대로다.
    let _ = std::fs::remove_dir(&revival.folder);
}

#[cfg(test)]
mod tests {
    use paddock_core::agent_account::AccountProfile;

    use super::*;
    use crate::test_support::TempHome;

    #[test]
    fn test_c_d40_a_shell_in_a_vanished_folder_opens_without_a_start_folder() {
        let home = TempHome::new();
        let there = home.dir.path().to_string_lossy().into_owned();
        let shell = |cwd: &str| RestorePlan {
            target: RestoreTarget::Shell { shell: "bash".into(), wsl_distribution: None, cwd: Some(cwd.into()) },
            command: None,
            notice: String::new(),
        };
        assert!(matches!(want_of(&shell(&there)), Want::New { cwd: Some(folder), .. } if folder == there));
        assert!(matches!(want_of(&shell("/definitely/not/here")), Want::New { cwd: None, .. }));
        // WSL 안의 경로는 이 컴퓨터에서 확인할 수 없어 그대로 둔다.
        let wsl = RestorePlan {
            target: RestoreTarget::Shell {
                shell: "wsl".into(),
                wsl_distribution: Some("Ubuntu".into()),
                cwd: Some("/home/me".into()),
            },
            command: None,
            notice: String::new(),
        };
        assert!(matches!(want_of(&wsl), Want::New { cwd: Some(_), .. }));
    }

    #[test]
    fn test_c_d40_an_account_terminal_resumes_in_its_own_account_folder_or_falls_back_to_the_recorded_launch() {
        let home = TempHome::new();
        let folder = home.dir.path().to_string_lossy().into_owned();
        let profile = AccountProfile {
            id: "00000000-0000-4000-8000-000000000001".into(),
            provider: "claude".into(),
            label: "Main".into(),
            runtime: Runtime::Native,
            wsl_distribution: None,
        };
        let launch = Launch {
            program: "/bin/bash".into(),
            args: vec!["-c".into(), "recorded".into()],
            cwd: Some(folder),
            title: Some("Claude \u{b7} Main".into()),
            account: Some(profile),
            ..Launch::default()
        };
        let session = "0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10";
        let plan = |resume: Option<&str>| RestorePlan {
            target: RestoreTarget::Account { launch: launch.clone(), resume_session_id: resume.map(str::to_string) },
            command: None,
            notice: String::new(),
        };
        let Want::Launch { launch: resumed, .. } = want_of(&plan(Some(session))) else { panic!("an account launch") };
        let Want::Launch { launch: same, .. } = want_of(&plan(None)) else { panic!("an account launch") };
        // 시작 명령(Windows는 인코딩된 PowerShell 명령)이 달라져야 하고, 이어 열 대화는 Unix에서 읽을 수 있는 시작 스크립트에 들어 있다.
        assert_ne!(resumed.args, same.args, "the conversation is resumed");
        #[cfg(unix)]
        {
            assert!(resumed.args.last().unwrap().contains(session));
            assert!(!same.args.last().unwrap().contains("--resume"), "nothing to resume");
        }
    }

    #[test]
    fn test_c_d46_finish_deletes_only_the_records_that_were_restored() {
        let home = TempHome::new();
        let folder = home.dir.path().join("old-daemon");
        std::fs::create_dir_all(&folder).unwrap();
        for id in [1, 2] {
            std::fs::write(folder.join(format!("{id}.json")), "{}").unwrap();
        }
        let revival = Revival { saved: SessionState::default(), records: BTreeMap::new(), folder: folder.clone() };
        finish(revival, &BTreeSet::from([1]));
        assert!(!folder.join("1.json").exists(), "the restored record is deleted");
        assert!(folder.join("2.json").exists(), "a record that could not be restored stays");
        let revival = Revival { saved: SessionState::default(), records: BTreeMap::new(), folder: folder.clone() };
        finish(revival, &BTreeSet::from([2]));
        assert!(!folder.exists(), "an emptied folder is removed");
    }

    #[test]
    fn test_c_d46_an_account_terminal_in_a_vanished_folder_still_resumes_and_falls_back_to_its_real_runtime() {
        let home = TempHome::new();
        let profile = |runtime: Runtime| AccountProfile {
            id: "00000000-0000-4000-8000-000000000001".into(),
            provider: "claude".into(),
            label: "Main".into(),
            runtime,
            wsl_distribution: (runtime == Runtime::Wsl).then(|| "Ubuntu".to_string()),
        };
        let session = "0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10";
        let launch = |runtime: Runtime, program: &str| Launch {
            program: program.into(),
            args: vec!["-c".into(), "recorded".into()],
            cwd: Some(home.dir.path().join("gone").to_string_lossy().into_owned()),
            title: Some("Claude \u{b7} Main".into()),
            account: Some(profile(runtime)),
            ..Launch::default()
        };
        let plan = |launch: Launch| RestorePlan {
            target: RestoreTarget::Account { launch, resume_session_id: Some(session.to_string()) },
            command: None,
            notice: String::new(),
        };
        // 폴더가 사라져도 시작 폴더만 빼고 같은 방식으로 이어 연다.
        let Want::Launch { launch: reopened, .. } = want_of(&plan(launch(Runtime::Native, "/bin/bash"))) else {
            panic!("an account launch")
        };
        assert!(
            reopened.cwd.as_deref().is_none_or(|folder| !folder.ends_with("gone")),
            "a folder that is gone is not asked for again: {:?}",
            reopened.cwd
        );
        #[cfg(unix)]
        assert!(reopened.args.last().unwrap().contains(session), "the conversation is still resumed");
        // 계정을 더 쓸 수 없어 기록된 명세로 열 때의 셸 이름은 계정의 실제 실행 환경이다.
        assert_eq!(fallback_shell(&launch(Runtime::Wsl, "wsl.exe")), "wsl");
        assert_eq!(
            fallback_shell(&launch(Runtime::Native, "powershell.exe")),
            default_shell_name(cfg!(windows), false)
        );
    }

    #[test]
    fn test_c_d46_a_restored_account_terminal_without_saved_state_gets_its_account_title() {
        assert_eq!(revived_name(Some("Claude \u{b7} Main"), || "terminal 4".to_string()), "Claude \u{b7} Main");
        assert_eq!(revived_name(None, || "terminal 4".to_string()), "terminal 4");
        assert_eq!(revived_name(Some(""), || "terminal 4".to_string()), "terminal 4");
    }
}
