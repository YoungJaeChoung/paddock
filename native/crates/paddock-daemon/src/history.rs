//! 터미널 기록(terminal-history)을 파일로 남기는 곳.
//!
//! 앱을 닫아도 터미널은 데몬에 남지만, 데몬이 크래시·강제 종료되거나 컴퓨터가 다시 켜지면 터미널 프로세스도 사라진다.
//! 그때 앱이 같은 폴더·같은 대화로 터미널을 되살릴 수 있도록, 데몬이 터미널마다 마지막 화면과 실행 정보를
//! `~/.paddock/native/terminal-history/<데몬 식별>/<터미널 번호>.json`에 남긴다.
//!
//! 규칙(저장 시점·기록 모양·오래된 기록 판정)은 `paddock-core`의 순수 함수이고, 이 모듈은 파일 쓰기·지우기와 에이전트 대화 찾기만 맡는다.
//! 화면 출력은 비밀이 찍혔을 수 있어 폴더는 소유자 전용으로 만들고, 데몬 기록 파일(daemon.log)에는 어떤 내용도 쓰지 않는다.
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use paddock_core::agent_account::{AccountProfile, Runtime};
use paddock_core::agent_resume::{
    AgentConversation, AgentFiles, AgentResume, CodexFiles, Foreground, claude_session_from_text, resumable_agent,
};
use paddock_core::settings_text::RestoreOnRestart;
use paddock_core::terminal_history::{HistoryLimits, HistoryRecord, directory_name, is_expired};
use paddock_protocol::daemon_info::{ensure_private_dir, write_private_file};
use paddock_protocol::shared_config::{history_root, native_profile_directory, read_settings};

use crate::probe::Reading;

/// 기록이 저장 시점에 이르렀는지 확인하는 간격. 저장 시점 자체(멎고 2초·최대 10초)는 core가 정한다.
pub const CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// 쓴 기록의 장부: 사용자가 끝내서 지운 터미널과, 터미널마다 지금까지 쓴 가장 새 기록의 세대 번호.
#[derive(Default)]
struct Ledger {
    /// 사용자가 끝내서 기록을 지운 터미널. 저장 중이던 기록이 지운 뒤에 되살아나지 않게 한다. 번호는 데몬 안에서 다시 쓰이지 않는다.
    removed: HashSet<u32>,
    /// 터미널의 기록을 만든 차례(세대 번호) 중 파일에 쓴 가장 큰 값. 이보다 오래된 기록은 쓰지 않는다.
    newest: HashMap<u32, u64>,
}

/// 이 데몬의 기록 폴더와, 데몬이 끝나는 중인지·쓴 기록의 장부.
pub struct History {
    /// 모든 데몬의 기록 폴더가 든 루트.
    root: PathBuf,
    /// 이 데몬의 기록 폴더(`root` 아래).
    directory: PathBuf,
    /// 데몬이 끝나는 중이다. 이때 끝나는 터미널의 기록은 지우지 않고 남긴다 — 다시 열 때 되살린다.
    stopping: AtomicBool,
    /// 파일을 쓰는 동안 쥔다. 같은 터미널의 두 쓰기가 겹쳐 오래된 기록이 새 기록을 덮지 못하게 한다.
    ledger: Mutex<Ledger>,
}

impl History {
    /// `identity`(데몬 식별 글자) 데몬의 기록 폴더를 가리킨다. 기록 루트는 홈 폴더 아래 고정 위치이고, 폴더는 처음 저장할 때 만든다.
    pub fn new(identity: &str) -> Self {
        let root = history_root();
        let directory = root.join(directory_name(identity));
        Self { root, directory, stopping: AtomicBool::new(false), ledger: Mutex::new(Ledger::default()) }
    }

    /// 데몬이 끝나는 중인가.
    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    /// 데몬이 끝나는 중임을 알린다. 이후 끝나는 터미널의 기록은 남는다.
    pub fn begin_stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
    }

    fn file_of(
        &self,
        id: u32,
    ) -> PathBuf {
        self.directory.join(format!("{id}.json"))
    }

    /// 터미널 `id`의 `generation`번째 기록을 쓴다. 이미 지운 터미널이거나 더 새로운 기록이 이미 쓰였으면 쓰지 않는다.
    /// 폴더는 소유자만, 파일도 소유자만 읽고 쓴다.
    pub fn save(
        &self,
        id: u32,
        generation: u64,
        record: &HistoryRecord,
    ) -> std::io::Result<()> {
        let mut ledger = self.ledger.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let is_stale =
            ledger.removed.contains(&id) || ledger.newest.get(&id).is_some_and(|newest| *newest > generation);
        let written = if is_stale {
            Ok(())
        } else {
            ensure_private_dir(&self.directory).and_then(|()| {
                let text = serde_json::to_vec(record).map_err(std::io::Error::other)?;
                write_private_file(&self.file_of(id), &text)
            })
        };
        if written.is_ok() && !is_stale {
            ledger.newest.insert(id, generation);
        }
        written
    }

    /// 터미널 `id`의 기록을 지운다. 없으면 아무 일도 하지 않는다.
    pub fn remove(
        &self,
        id: u32,
    ) {
        let mut ledger = self.ledger.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        ledger.removed.insert(id);
        let _ = std::fs::remove_file(self.file_of(id));
    }

    /// 모든 데몬의 기록을 지운다(되살리기를 끈 설정).
    pub fn clear_all(&self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }

    /// 14일 넘게 갱신되지 않은 다른 데몬의 기록 폴더를 지운다. 이 데몬의 폴더는 건드리지 않는다. 지운 폴더 수를 돌려준다.
    pub fn prune_old(&self) -> usize {
        let mut n_removed = 0;
        for entry in std::fs::read_dir(&self.root).into_iter().flatten().flatten() {
            let path = entry.path();
            if path != self.directory
                && path.is_dir()
                && newest_age(&path).is_some_and(|age| is_expired(u64::try_from(age.as_millis()).unwrap_or(u64::MAX)))
                && std::fs::remove_dir_all(&path).is_ok()
            {
                n_removed += 1;
            }
        }
        n_removed
    }
}

/// 폴더 안 파일 중 가장 최근에 바뀐 것(파일이 없으면 폴더 자신)이 바뀐 뒤 지난 시간.
fn newest_age(directory: &Path) -> Option<Duration> {
    let modified = |path: &Path| std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok();
    let newest = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| modified(&entry.path()))
        .max()
        .or_else(|| modified(directory));
    newest.map(|time| SystemTime::now().duration_since(time).unwrap_or_default())
}

/// 되살리기가 켜져 있는가. 설정은 저장할 때마다 읽는다.
pub fn is_enabled() -> bool {
    read_settings().restore_on_restart() == RestoreOnRestart::Always
}

/// 지금 앞쪽에서 도는 에이전트와 그 대화. `pid`는 대화를 찾은 프로세스라, 같은 프로세스에서는 다시 찾지 않는다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSeen {
    pub pid: u32,
    pub conversation: AgentConversation,
}

/// 대화를 찾는 데 쓰는 터미널 쪽 맥락.
pub struct AgentContext<'a> {
    /// 계정 버튼으로 연 터미널의 계정.
    pub opened_by: Option<&'a AccountProfile>,
    /// 셸이 알린 현재 폴더(운영체제가 알려 주지 않는 환경의 대비).
    pub reported_cwd: Option<&'a str>,
    /// 이미 찾은 대화.
    pub known: Option<&'a AgentSeen>,
}

/// 파일을 최대 `max_bytes`바이트까지 글자로 읽는다. 읽을 수 없으면 None이다.
fn read_capped(
    path: &Path,
    max_bytes: usize,
) -> Option<String> {
    let mut text = String::new();
    let file = std::fs::File::open(path).ok()?;
    file.take(max_bytes as u64 + 1).read_to_string(&mut text).ok()?;
    Some(text)
}

/// 이 컴퓨터의 파일 시스템에서 Codex rollout 파일을 찾는다.
struct DiskCodexFiles;

impl CodexFiles for DiskCodexFiles {
    fn read_dir(
        &self,
        directory: &Path,
    ) -> Option<Vec<String>> {
        let names = std::fs::read_dir(directory).ok()?;
        Some(names.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
    }

    fn read_first_line(
        &self,
        file: &Path,
        max_bytes: usize,
    ) -> Option<String> {
        let mut bytes = Vec::new();
        std::fs::File::open(file).ok()?.take(max_bytes as u64).read_to_end(&mut bytes).ok()?;
        let line = bytes.split(|byte| *byte == b'\n').next().unwrap_or_default();
        Some(String::from_utf8_lossy(line).into_owned())
    }
}

/// 앞쪽 프로그램이 이어 열 수 있는 에이전트이면 그 대화를 찾는다. 에이전트가 아니거나 대화를 확신할 수 없으면 None이다.
///
/// 같은 프로세스에서 이미 찾은 대화는 파일을 다시 읽지 않고 돌려준다. 못 찾았으면 다음 조사 때 다시 찾는다 —
/// Claude는 시작 직후 기록을 남기기까지, Codex는 첫 대화를 하기까지 파일이 없을 수 있다.
///
/// | 환경 | 대화를 읽는 곳 |
/// |------|----------------|
/// | Linux·Windows 네이티브 | 계정 폴더(프로세스 환경, 환경을 못 읽는 Windows는 계정 터미널의 계정 폴더)의 sessions·rollout |
/// | Windows의 WSL | 조사가 WSL 안에서 읽어 온 Claude sessions 기록 본문(Codex는 찾지 않는다) |
pub fn resolve_agent(
    reading: &Reading,
    program: Option<&str>,
    context: &AgentContext,
) -> Option<AgentSeen> {
    let pid = reading.pid?;
    let name = program.filter(|name| HistoryLimits::AGENTS.contains(name))?;
    let cached = context.known.filter(|seen| seen.pid == pid && seen.conversation.provider == name);
    let found = match cached {
        Some(seen) => Some(seen.conversation.clone()),
        None if reading.runtime == Runtime::Wsl => wsl_conversation(reading, name, pid),
        None => native_conversation(reading, name, pid, context),
    };
    found.map(|conversation| AgentSeen { pid, conversation })
}

/// WSL 안에서 읽어 온 Claude sessions 기록 본문으로 대화를 찾는다.
fn wsl_conversation(
    reading: &Reading,
    name: &str,
    pid: u32,
) -> Option<AgentConversation> {
    let config_dir = reading.env.as_ref().and_then(|env| env.claude_config_dir.clone());
    let found = reading
        .session_record
        .as_deref()
        .filter(|_| name == "claude")
        .and_then(|text| claude_session_from_text(text, pid, reading.started_at))?;
    Some(AgentConversation {
        provider: "claude".to_string(),
        session_id: found.session_id,
        cwd: if found.cwd.is_empty() { reading.cwd.clone().unwrap_or_default() } else { found.cwd },
        config_dir,
    })
}

/// 이 컴퓨터의 계정 폴더·홈 폴더 아래 파일로 대화를 찾는다.
fn native_conversation(
    reading: &Reading,
    name: &str,
    pid: u32,
    context: &AgentContext,
) -> Option<AgentConversation> {
    let env = reading.env.as_ref();
    // 프로세스 환경을 읽지 못하는 환경(Windows 네이티브)에서는 계정 터미널이 쓰는 계정 폴더를 알려진 값으로 쓴다.
    let account_dir = context
        .opened_by
        .filter(|profile| profile.runtime == Runtime::Native && profile.provider == name && env.is_none())
        .map(native_profile_directory);
    let home_directory = std::env::home_dir().unwrap_or_default();
    let read_file = |path: &Path| read_capped(path, AgentResume::N_MAX_RECORD_BYTES);
    let files = AgentFiles { home_directory: &home_directory, read_file: &read_file, codex: &DiskCodexFiles };
    let cwd = reading.cwd.as_deref().or(context.reported_cwd).unwrap_or("");
    let foreground = Foreground {
        program: name,
        pid,
        home: env.and_then(|env| env.home.as_deref()),
        claude_config_dir: env
            .and_then(|env| env.claude_config_dir.as_deref())
            .or(account_dir.as_deref().filter(|_| name == "claude")),
        codex_home: env
            .and_then(|env| env.codex_home.as_deref())
            .or(account_dir.as_deref().filter(|_| name == "codex")),
        cwd,
        started_at: reading.started_at,
    };
    resumable_agent(&foreground, &files)
}

#[cfg(test)]
mod tests {
    use paddock_core::agent_account::AccountEnv;
    use paddock_core::terminal_history::{HistoryInput, build_record};

    use super::*;
    use crate::test_home::TempHome;

    const SESSION: &str = "0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10";

    fn record(output: &str) -> HistoryRecord {
        build_record(&HistoryInput {
            saved_at: 1,
            shell: "bash",
            wsl_distribution: None,
            launch: None,
            cwd_start: Some("/work"),
            cwd_now: None,
            program: None,
            agent: None,
            output: output.as_bytes(),
        })
    }

    #[test]
    fn test_c_d40_a_saved_record_is_replaced_removed_and_never_written_again_after_removal() {
        let home = TempHome::new();
        let root = history_root();
        let history = History::new("7@2026-10-16T21:00:00+09:00");
        history.save(3, 1, &record("first")).unwrap();
        history.save(3, 2, &record("second")).unwrap();
        let folder = root.join("7@2026-10-16T21-00-00-09-00");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1, "no temporary file stays");
        let text = std::fs::read_to_string(folder.join("3.json")).unwrap();
        assert_eq!(paddock_core::terminal_history::parse_record(&text).unwrap().output, "second");
        history.remove(3);
        assert!(!folder.join("3.json").exists());
        history.save(3, 3, &record("late")).unwrap();
        assert!(
            !folder.join("3.json").exists(),
            "a save that was already in flight cannot bring a removed record back"
        );
        history.remove(99);
        drop(home);
    }

    #[test]
    fn test_c_d46_an_older_write_cannot_replace_a_newer_record_of_the_same_terminal() {
        let _home = TempHome::new();
        let history = History::new("own");
        history.save(5, 2, &record("newest")).unwrap();
        history.save(5, 1, &record("late and stale")).unwrap();
        let text = std::fs::read_to_string(history_root().join("own").join("5.json")).unwrap();
        assert_eq!(paddock_core::terminal_history::parse_record(&text).unwrap().output, "newest");
        history.save(5, 3, &record("newer still")).unwrap();
        let text = std::fs::read_to_string(history_root().join("own").join("5.json")).unwrap();
        assert_eq!(paddock_core::terminal_history::parse_record(&text).unwrap().output, "newer still");
    }

    /// 파일의 바뀐 시각을 바꾼다.
    fn set_modified(
        path: &Path,
        time: SystemTime,
    ) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(time).unwrap();
    }

    #[test]
    fn test_c_d40_only_other_daemons_folders_older_than_14_days_are_pruned() {
        let home = TempHome::new();
        let root = history_root();
        let history = History::new("own");
        history.save(1, 1, &record("mine")).unwrap();
        let day = Duration::from_secs(24 * 60 * 60);
        let make = |name: &str, age_days: u64| {
            let folder = root.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("1.json"), "{}").unwrap();
            // 폴더 자신의 시각은 파일이 있으면 보지 않으므로 파일 시각만 바꾼다(Windows는 폴더를 파일처럼 열 수 없다).
            set_modified(&folder.join("1.json"), SystemTime::now() - day * age_days as u32);
        };
        make("stale", 15);
        make("recent", 13);
        // 오래된 폴더라도 이 데몬의 것이면 지우지 않는다.
        let own = root.join("own");
        set_modified(&own.join("1.json"), SystemTime::now() - day * 30);
        assert_eq!(history.prune_old(), 1);
        assert!(!root.join("stale").exists());
        assert!(root.join("recent").exists() && own.exists());
        drop(home);
    }

    #[test]
    fn test_c_d40_stopping_is_remembered() {
        let _home = TempHome::new();
        let history = History::new("own");
        assert!(!history.is_stopping());
        history.begin_stop();
        assert!(history.is_stopping());
    }

    fn reading(
        runtime: Runtime,
        pid: Option<u32>,
    ) -> Reading {
        Reading {
            cwd: Some("/work".to_string()),
            argv: vec!["claude".to_string()],
            env: Some(AccountEnv::default()),
            runtime,
            distribution: String::new(),
            pid,
            started_at: None,
            session_record: None,
        }
    }

    fn context<'a>() -> AgentContext<'a> {
        AgentContext { opened_by: None, reported_cwd: None, known: None }
    }

    #[test]
    fn test_c_d40_only_claude_and_codex_with_a_known_pid_get_a_conversation_lookup() {
        let with_pid = reading(Runtime::Native, Some(42));
        assert_eq!(resolve_agent(&with_pid, Some("bash"), &context()), None);
        assert_eq!(resolve_agent(&with_pid, None, &context()), None);
        assert_eq!(resolve_agent(&reading(Runtime::Native, None), Some("claude"), &context()), None);
    }

    #[test]
    fn test_c_d40_a_conversation_found_for_a_process_is_kept_while_the_process_runs() {
        let known = AgentSeen {
            pid: 42,
            conversation: AgentConversation {
                provider: "claude".into(),
                session_id: SESSION.into(),
                cwd: "/work".into(),
                config_dir: None,
            },
        };
        let same = resolve_agent(
            &reading(Runtime::Native, Some(42)),
            Some("claude"),
            &AgentContext { known: Some(&known), ..context() },
        );
        assert_eq!(same, Some(known.clone()), "no file is read again for the same process");
        // 다른 프로세스는 이어받지 않는다(이 시험 환경에는 그 pid의 sessions 기록이 없다).
        let other = resolve_agent(
            &reading(Runtime::Native, Some(43)),
            Some("claude"),
            &AgentContext { known: Some(&known), ..context() },
        );
        assert_eq!(other, None);
    }

    #[test]
    fn test_c_d40_a_wsl_agent_conversation_comes_from_the_record_read_inside_wsl() {
        let mut wsl = reading(Runtime::Wsl, Some(42));
        wsl.env =
            Some(AccountEnv { claude_config_dir: Some("/home/me/.paddock/acc/claude".into()), ..Default::default() });
        wsl.session_record =
            Some(format!(r#"{{"pid":42,"sessionId":"{SESSION}","kind":"interactive","cwd":"/home/me/app"}}"#));
        let found = resolve_agent(&wsl, Some("claude"), &context()).expect("a conversation");
        assert_eq!(
            (found.pid, found.conversation.session_id.as_str(), found.conversation.cwd.as_str()),
            (42, SESSION, "/home/me/app")
        );
        assert_eq!(found.conversation.config_dir.as_deref(), Some("/home/me/.paddock/acc/claude"));
        // 같은 기록이라도 다른 pid의 것이면 쓰지 않는다. Codex는 WSL 안의 rollout 파일을 읽지 못해 찾지 않는다.
        wsl.pid = Some(43);
        assert_eq!(resolve_agent(&wsl, Some("claude"), &context()), None);
        wsl.pid = Some(42);
        assert_eq!(resolve_agent(&wsl, Some("codex"), &context()), None);
    }
}
