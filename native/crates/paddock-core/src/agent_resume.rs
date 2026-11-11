//! 터미널에서 실행 중인 claude·codex가 어느 대화를 열고 있는지 읽어, 터미널을 다시 열 때 그 대화를 이어 열 수 있게 하는 규칙.
//!
//! 대화 식별자는 Claude가 프로세스마다 남기는 sessions 기록과 Codex가 대화마다 남기는 rollout 파일의 첫 줄에서 읽는다.
//! 대화 내용은 읽지 않는다. 파일 읽기는 호출자가 넘기는 함수·객체가 하므로, 실제 홈 폴더 없이 시험한다.
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Local, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::work_model::same_folder;

/// 대화 찾기의 한도와 여유.
pub struct AgentResume;

impl AgentResume {
    /// sessions 기록 파일 하나의 최대 크기. 이보다 크면 읽지 않는다.
    pub const N_MAX_RECORD_BYTES: usize = 16384;
    /// rollout 파일에서 첫 줄을 읽을 최대 바이트. 첫 줄에 지시문 전문이 들어가 길 수 있다.
    pub const N_FIRST_LINE_BYTES: usize = 65536;
    /// 프로세스 시작 시각을 초 단위로만 알 수 있어 두는 여유(ms). 대화 파일이 이 시각보다 이만큼 일러도 같은 프로세스의 것으로 본다.
    pub const START_MARGIN_MS: u64 = 3000;
    /// Claude가 프로세스를 시작한 뒤 sessions 기록을 남기기까지 걸릴 수 있는 최대 시간(ms).
    pub const RECORD_LAG_MS: u64 = 120_000;
}

/// 이어 열 수 있는 에이전트 대화 하나.
///
/// `config_dir`는 에이전트를 계정 폴더 환경 변수로 지정해 실행했을 때만 있다. 같은 폴더로 이어 열어야 같은 대화를 찾는다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversation {
    pub provider: String,
    pub session_id: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
}

/// Claude sessions 기록에서 읽은 대화.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaudeSession {
    pub session_id: String,
    pub cwd: String,
}

/// rollout 첫 줄에서 읽은 Codex 대화의 머리 정보.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexHeader {
    pub id: String,
    pub cwd: String,
    /// 대화를 시작한 시각(ms, 1970년 기준).
    pub started_at: u64,
}

/// 대화 식별자(UUID)의 모양. Claude는 UUID v4, Codex는 v7이라 버전 칸은 가리지 않는다.
pub fn is_session_id(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let lengths = [8, 4, 4, 4, 12];
    parts.len() == lengths.len()
        && parts.iter().zip(lengths).all(|(part, length)| {
            part.chars().count() == length && part.chars().all(|character| character.is_ascii_hexdigit())
        })
}

/// Claude sessions 기록 본문에서 이 프로세스의 대화 식별자를 읽는다. 이 프로세스의 것이 아니거나 깨졌으면 None이다.
///
/// 기록의 `pid`가 같고 `kind`가 interactive(터미널에서 쓰는 대화형)일 때만 인정한다 — 하위 에이전트·비대화형 실행은 터미널의 대화가 아니다.
/// WSL 안의 기록처럼 본문만 읽힌 경우에도 같은 판정을 쓴다.
///
/// Examples
/// --------
/// | sessions/42.json                                             | pid | 결과           |
/// |--------------------------------------------------------------|-----|----------------|
/// | `{"pid":42,"sessionId":"<uuid>","kind":"interactive"}`       | 42  | 그 대화        |
/// | 같은 기록                                                    | 43  | None           |
/// | `"kind":"sdk"`                                               | 42  | None           |
/// | 깨진 JSON                                                    | 42  | None           |
/// | 기록의 `startedAt`이 프로세스 시작보다 오래전                | 42  | None           |
///
/// `process_started_at`(ms)을 모르면 시작 시각 비교는 건너뛴다.
pub fn claude_session_from_text(
    text: &str,
    pid: u32,
    process_started_at: Option<u64>,
) -> Option<ClaudeSession> {
    let record =
        (text.len() <= AgentResume::N_MAX_RECORD_BYTES).then(|| serde_json::from_str::<Value>(text).ok()).flatten();
    record.and_then(|record| {
        let started_at = record.get("startedAt").and_then(Value::as_f64);
        // pid가 재사용되면 옛 프로세스의 기록이 남아 있을 수 있다. 기록의 시작 시각이 이 프로세스의 시작과 맞지 않으면 다른 프로세스의 기록이다.
        let is_same_process = match (process_started_at, started_at) {
            (Some(process), Some(recorded)) => {
                recorded >= process as f64 - AgentResume::START_MARGIN_MS as f64
                    && recorded <= process as f64 + AgentResume::RECORD_LAG_MS as f64
            }
            _ => true,
        };
        let session_id = record.get("sessionId").and_then(Value::as_str).filter(|id| is_session_id(id));
        let is_interactive = record.get("kind").and_then(Value::as_str) == Some("interactive");
        let is_pid = record.get("pid").and_then(Value::as_u64) == Some(u64::from(pid));
        let cwd = record.get("cwd").and_then(Value::as_str).unwrap_or("");
        (is_pid && is_interactive && is_same_process)
            .then(|| session_id.map(|id| ClaudeSession { session_id: id.to_string(), cwd: cwd.to_string() }))
            .flatten()
    })
}

/// 계정 폴더 아래 `sessions/<pid>.json`을 읽어 `claude_session_from_text`로 판정한다. 파일이 없으면 None이다.
pub fn claude_session_of(
    config_directory: &Path,
    pid: u32,
    read_file: &dyn Fn(&Path) -> Option<String>,
    process_started_at: Option<u64>,
) -> Option<ClaudeSession> {
    let record_path = config_directory.join("sessions").join(format!("{pid}.json"));
    read_file(&record_path).and_then(|text| claude_session_from_text(&text, pid, process_started_at))
}

/// rollout 파일의 첫 줄(JSON)에서 이어 열 수 있는 대화인지 판정한다. 아니면 None이다.
/// 터미널에서 직접 연 Codex 대화(originator가 `codex-tui`, source가 문자열)만 인정한다 — 하위 에이전트(source가 객체)·비대화형 실행(`codex_exec`)은 제외한다.
pub fn codex_session_header(first_line: &str) -> Option<CodexHeader> {
    let line: Value = serde_json::from_str(first_line).ok()?;
    let payload = line.get("payload")?;
    let text_of = |name: &str| payload.get(name).and_then(Value::as_str);
    let started_at = text_of("timestamp")
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .and_then(|time| u64::try_from(time.timestamp_millis()).ok());
    let id = text_of("id").filter(|id| is_session_id(id));
    let cwd = text_of("cwd").filter(|cwd| !cwd.is_empty());
    let is_terminal = text_of("originator") == Some("codex-tui") && text_of("source").is_some();
    match (is_terminal, id, cwd, started_at) {
        (true, Some(id), Some(cwd), Some(started_at)) => {
            Some(CodexHeader { id: id.to_string(), cwd: cwd.to_string(), started_at })
        }
        _ => None,
    }
}

/// Codex 프로세스가 연 대화를 후보 중에서 고른다. 후보가 정확히 하나일 때만 고르고, 아니면 None이다.
/// 후보 조건은 작업 폴더가 같고 시작 시각이 프로세스 시작 이후(여유 3초)인 것이다.
/// 같은 폴더에서 연 대화가 둘 이상이면 어느 것이 이 터미널의 것인지 정할 수 없어 고르지 않는다 — 틀린 대화를 이어 여는 것보다 이어 열지 않는 편이 낫다.
///
/// Examples
/// --------
/// | 후보(폴더, 시작)                          | 프로세스(폴더, 시작) | 결과      |
/// |-------------------------------------------|----------------------|-----------|
/// | (a, 100s)                                 | (a, 90s)             | 그 대화   |
/// | (a, 100s) · (a, 120s)                     | (a, 90s)             | None      |
/// | (b, 100s)                                 | (a, 90s)             | None      |
/// | (a, 50s)                                  | (a, 90s)             | None      |
pub fn choose_codex_session(
    candidates: &[CodexHeader],
    cwd: &str,
    process_started_at: u64,
) -> Option<ClaudeSession> {
    let earliest = process_started_at.saturating_sub(AgentResume::START_MARGIN_MS);
    let mut matches = candidates.iter().filter(|item| same_folder(&item.cwd, cwd) && item.started_at >= earliest);
    match (matches.next(), matches.next()) {
        (Some(only), None) => Some(ClaudeSession { session_id: only.id.clone(), cwd: only.cwd.clone() }),
        _ => None,
    }
}

/// rollout 파일을 찾는 데 쓰는 파일 읽기.
pub trait CodexFiles {
    /// 폴더 안 이름들. 폴더가 없으면 None이다.
    fn read_dir(
        &self,
        directory: &Path,
    ) -> Option<Vec<String>>;

    /// 파일의 첫 줄(최대 `max_bytes`바이트). 읽을 수 없으면 None이다.
    fn read_first_line(
        &self,
        file: &Path,
        max_bytes: usize,
    ) -> Option<String>;
}

/// Codex가 프로세스 시작 이후 만든 rollout 파일들의 머리 정보를 읽는다. 시작한 날과 다음 날 폴더(자정을 넘긴 경우)만 본다.
/// 폴더가 없으면 빈 목록이다.
pub fn list_codex_sessions(
    codex_home: &Path,
    process_started_at: u64,
    files: &dyn CodexFiles,
) -> Vec<CodexHeader> {
    const DAY_MS: u64 = 24 * 60 * 60 * 1000;
    let mut headers: Vec<CodexHeader> = Vec::new();
    for millis in [process_started_at, process_started_at + DAY_MS] {
        // Codex는 파일이 놓이는 날짜 폴더에 로컬 시각을 쓴다.
        let day = i64::try_from(millis).ok().and_then(|millis| Local.timestamp_millis_opt(millis).single());
        let directory = day.map(|day| {
            codex_home
                .join("sessions")
                .join(format!("{:04}", day.year()))
                .join(format!("{:02}", day.month()))
                .join(format!("{:02}", day.day()))
        });
        let names = directory.as_deref().and_then(|directory| files.read_dir(directory)).unwrap_or_default();
        for name in names.iter().filter(|name| name.starts_with("rollout-") && name.ends_with(".jsonl")) {
            let first_line = directory
                .as_ref()
                .and_then(|directory| files.read_first_line(&directory.join(name), AgentResume::N_FIRST_LINE_BYTES));
            if let Some(header) = first_line.and_then(|line| codex_session_header(&line))
                && !headers.iter().any(|known| known.id == header.id)
            {
                headers.push(header);
            }
        }
    }
    headers
}

/// `ps -o etime=` 값(`[[일-]시:]분:초`)을 초로 바꾼다. 형식이 맞지 않으면 None이다.
///
/// Examples
/// --------
/// | 입력          | 결과  |
/// |---------------|-------|
/// | `05:07`       | 307   |
/// | `01:02:03`    | 3723  |
/// | `2-00:00:01`  | 172801 |
/// | `abc`         | None  |
pub fn elapsed_seconds(text: &str) -> Option<u64> {
    let is_number = |part: &str| !part.is_empty() && part.chars().all(|character| character.is_ascii_digit());
    let trimmed = text.trim();
    let (days, clock) = match trimmed.split_once('-') {
        Some((days, clock)) if is_number(days) => (days.parse::<u64>().ok()?, clock),
        _ => (0, trimmed),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    let numbers: Option<Vec<u64>> =
        parts.iter().map(|part| is_number(part).then(|| part.parse::<u64>().ok()).flatten()).collect();
    match numbers.as_deref() {
        Some([minutes, seconds]) => Some((days * 24 * 60 + minutes) * 60 + seconds),
        Some([hours, minutes, seconds]) => Some(((days * 24 + hours) * 60 + minutes) * 60 + seconds),
        _ => None,
    }
}

/// 터미널 앞쪽에서 실행 중인 프로그램.
#[derive(Clone, Debug)]
pub struct Foreground<'a> {
    pub program: &'a str,
    pub pid: u32,
    /// 프로그램 환경의 홈 폴더. 모르면 None이라 `AgentFiles::home_directory`를 쓴다.
    pub home: Option<&'a str>,
    /// 계정 폴더를 지정해 실행했을 때 그 폴더(Claude).
    pub claude_config_dir: Option<&'a str>,
    /// 계정 폴더를 지정해 실행했을 때 그 폴더(Codex).
    pub codex_home: Option<&'a str>,
    /// 셸의 현재 폴더.
    pub cwd: &'a str,
    /// 프로세스 시작 시각(ms). 모르면 None.
    pub started_at: Option<u64>,
}

/// 대화를 찾을 때 쓰는 파일 읽기와 홈 폴더.
pub struct AgentFiles<'a> {
    pub home_directory: &'a Path,
    pub read_file: &'a dyn Fn(&Path) -> Option<String>,
    pub codex: &'a dyn CodexFiles,
}

/// 앞쪽 프로그램이 이어 열 수 있는 에이전트일 때 그 대화를 돌려준다. 아니면 None이다.
/// 에이전트가 아니거나 대화를 확신할 수 없으면 None이므로, 호출한 쪽은 이어 열지 않고 새 셸만 연다.
///
/// 시작 시각은 Codex 대화를 고를 때 꼭 필요하고(모르면 Codex는 None), Claude 기록이 이 프로세스의 것인지 확인할 때는 알 때만 쓴다.
pub fn resumable_agent(
    foreground: &Foreground,
    files: &AgentFiles,
) -> Option<AgentConversation> {
    let home = foreground.home.map_or_else(|| files.home_directory.to_path_buf(), PathBuf::from);
    let mut result = None;
    if foreground.program == "claude" && foreground.pid > 0 {
        let directory = foreground.claude_config_dir.map_or_else(|| home.join(".claude"), PathBuf::from);
        let found = claude_session_of(&directory, foreground.pid, files.read_file, foreground.started_at);
        // 계정 폴더를 환경 변수로 지정해 실행한 경우만 남긴다 — 복원 셸이 같은 계정 폴더로 --resume해야 대화를 찾는다.
        result = found.map(|found| AgentConversation {
            provider: "claude".to_string(),
            session_id: found.session_id,
            cwd: if found.cwd.is_empty() { foreground.cwd.to_string() } else { found.cwd },
            config_dir: foreground.claude_config_dir.map(str::to_string),
        });
    } else if foreground.program == "codex"
        && !foreground.cwd.is_empty()
        && let Some(started_at) = foreground.started_at
    {
        let codex_home = foreground.codex_home.map_or_else(|| home.join(".codex"), PathBuf::from);
        let candidates = list_codex_sessions(&codex_home, started_at, files.codex);
        result = choose_codex_session(&candidates, foreground.cwd, started_at).map(|found| AgentConversation {
            provider: "codex".to_string(),
            session_id: found.session_id,
            cwd: found.cwd,
            config_dir: None,
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;

    const CLAUDE_ID: &str = "0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10";
    const CODEX_ID: &str = "01a0ae67-7d1b-7ba0-a75e-113eb195b380";
    const CODEX_ID_2: &str = "01a0ae67-7d1b-7ba0-a75e-113eb195b381";

    fn rollout(overrides: &[(&str, Value)]) -> String {
        let mut payload = serde_json::json!({
            "id": CODEX_ID, "cwd": "/work/app", "originator": "codex-tui", "source": "cli",
            "timestamp": "2026-10-14T01:00:10.000Z",
        });
        for (name, value) in overrides {
            payload[*name] = value.clone();
        }
        serde_json::json!({ "payload": payload }).to_string()
    }

    fn claude_record(extra: &[(&str, Value)]) -> String {
        let mut record =
            serde_json::json!({ "pid": 42, "sessionId": CLAUDE_ID, "kind": "interactive", "cwd": "/work/app" });
        for (name, value) in extra {
            record[*name] = value.clone();
        }
        record.to_string()
    }

    fn millis(text: &str) -> u64 {
        DateTime::parse_from_rfc3339(text).unwrap().timestamp_millis() as u64
    }

    /// 경로 → 본문 표로 읽는 가짜 파일 읽기.
    fn files_of(table: &[(&str, String)]) -> impl Fn(&Path) -> Option<String> + use<> {
        let map: BTreeMap<PathBuf, String> =
            table.iter().map(|(path, text)| (PathBuf::from(path), text.clone())).collect();
        move |path| map.get(path).cloned()
    }

    #[test]
    fn test_c_resume_1_only_a_record_with_the_same_pid_and_interactive_kind_counts() {
        let session = |pid: u32, text: &str| claude_session_from_text(text, pid, None);
        assert_eq!(
            session(42, &claude_record(&[])),
            Some(ClaudeSession { session_id: CLAUDE_ID.into(), cwd: "/work/app".into() })
        );
        assert_eq!(session(43, &claude_record(&[])), None);
        assert_eq!(session(42, &claude_record(&[("kind", "sdk".into())])), None);
        assert_eq!(session(42, &claude_record(&[("sessionId", "not-a-uuid".into())])), None);
        assert_eq!(session(42, "{broken"), None);
        assert_eq!(claude_session_of(Path::new("/c"), 42, &|_| None, None), None);
    }

    #[test]
    fn test_c_resume_2_a_record_that_started_far_from_the_process_start_belongs_to_another_process() {
        let at = millis("2026-10-14T01:00:00Z");
        let record = |started_at: i64| claude_record(&[("startedAt", started_at.into())]);
        let judged = |text: &str, started: Option<u64>| claude_session_from_text(text, 42, started);
        assert!(judged(&record(at as i64 + 5000), Some(at)).is_some());
        assert!(judged(&record(at as i64 - 3_600_000), Some(at)).is_none(), "an old record left by a reused pid");
        assert!(judged(&record(at as i64 + 3_600_000), Some(at)).is_none());
        // 시작 시각을 모르거나 기록에 없으면 비교하지 않는다.
        assert!(judged(&record(at as i64 - 3_600_000), None).is_some());
        assert!(judged(&claude_record(&[]), Some(at)).is_some());
    }

    #[test]
    fn test_c_resume_3_an_agent_run_with_an_account_folder_keeps_that_folder() {
        let read = files_of(&[("/acc/sessions/42.json", claude_record(&[]))]);
        let nothing = NoCodexFiles;
        let files = AgentFiles { home_directory: Path::new("/home/me"), read_file: &read, codex: &nothing };
        let foreground = Foreground {
            program: "claude",
            pid: 42,
            home: None,
            claude_config_dir: Some("/acc"),
            codex_home: None,
            cwd: "/w",
            started_at: None,
        };
        assert_eq!(
            resumable_agent(&foreground, &files),
            Some(AgentConversation {
                provider: "claude".into(),
                session_id: CLAUDE_ID.into(),
                cwd: "/work/app".into(),
                config_dir: Some("/acc".into()),
            })
        );
    }

    #[test]
    fn test_c_resume_4_the_record_is_read_from_sessions_under_the_account_folder() {
        let requested = RefCell::new(PathBuf::new());
        let read = |path: &Path| {
            *requested.borrow_mut() = path.to_path_buf();
            None
        };
        claude_session_of(Path::new("/acc"), 7, &read, None);
        assert_eq!(*requested.borrow(), Path::new("/acc").join("sessions").join("7.json"));
    }

    #[test]
    fn test_c_resume_5_only_a_codex_conversation_opened_in_a_terminal_counts() {
        assert_eq!(
            codex_session_header(&rollout(&[])),
            Some(CodexHeader {
                id: CODEX_ID.into(),
                cwd: "/work/app".into(),
                started_at: millis("2026-10-14T01:00:10Z")
            })
        );
        assert_eq!(
            codex_session_header(&rollout(&[("originator", "codex_exec".into()), ("source", "exec".into())])),
            None
        );
        assert_eq!(codex_session_header(&rollout(&[("source", serde_json::json!({"subagent": {}}))])), None);
        assert_eq!(codex_session_header(&rollout(&[("id", "x".into())])), None);
        assert_eq!(codex_session_header("{broken"), None);
    }

    #[test]
    fn test_c_resume_6_a_codex_conversation_is_chosen_only_when_exactly_one_matches() {
        let at = |seconds: i64| (millis("2026-10-14T01:00:00Z") as i64 + seconds * 1000) as u64;
        let item =
            |id: &str, cwd: &str, seconds: i64| CodexHeader { id: id.into(), cwd: cwd.into(), started_at: at(seconds) };
        let started = at(0);
        let chosen = |candidates: &[CodexHeader], cwd: &str| choose_codex_session(candidates, cwd, started);
        let only = ClaudeSession { session_id: "a".into(), cwd: "/w".into() };
        assert_eq!(chosen(&[item("a", "/w", 10)], "/w"), Some(only.clone()));
        assert_eq!(chosen(&[item("a", "/w", 10)], "/w/"), Some(only.clone()));
        assert_eq!(chosen(&[item("a", "/w", 10), item("b", "/w", 20)], "/w"), None);
        assert_eq!(chosen(&[item("a", "/other", 10)], "/w"), None);
        assert_eq!(chosen(&[item("a", "/w", -60)], "/w"), None);
        // 같은 폴더의 다른 표기(구분자·대소문자·끝 구분자)도 같은 폴더다. Linux 경로는 대소문자를 구분한다.
        let windows = [item("w", "C:\\Work\\App", 10)];
        assert_eq!(chosen(&windows, "c:/work/app/").map(|found| found.session_id), Some("w".to_string()));
        assert_eq!(chosen(&[item("l", "/Work", 10)], "/work"), None);
        // 초 단위로만 아는 프로세스 시작 시각의 오차(3초) 안이면 같은 프로세스의 대화로 본다.
        assert_eq!(chosen(&[item("a", "/w", -2)], "/w"), Some(only));
        assert_eq!(chosen(&[], "/w"), None);
    }

    struct FakeCodexFiles {
        directories: BTreeMap<PathBuf, Vec<String>>,
        asked: RefCell<Vec<PathBuf>>,
    }

    impl CodexFiles for FakeCodexFiles {
        fn read_dir(
            &self,
            directory: &Path,
        ) -> Option<Vec<String>> {
            self.asked.borrow_mut().push(directory.to_path_buf());
            self.directories.get(directory).cloned()
        }

        fn read_first_line(
            &self,
            file: &Path,
            _max_bytes: usize,
        ) -> Option<String> {
            let id = if file.to_string_lossy().ends_with("a.jsonl") { CODEX_ID } else { CODEX_ID_2 };
            Some(rollout(&[("id", id.into())]))
        }
    }

    struct NoCodexFiles;

    impl CodexFiles for NoCodexFiles {
        fn read_dir(
            &self,
            _directory: &Path,
        ) -> Option<Vec<String>> {
            None
        }

        fn read_first_line(
            &self,
            _file: &Path,
            _max_bytes: usize,
        ) -> Option<String> {
            None
        }
    }

    #[test]
    fn test_c_resume_7_codex_files_are_listed_from_the_start_day_and_the_next_day_only() {
        let started = Local.with_ymd_and_hms(2026, 10, 14, 23, 59, 0).unwrap().timestamp_millis() as u64;
        let mut directories = BTreeMap::new();
        directories.insert(
            PathBuf::from("/h/.codex/sessions/2026/10/14"),
            vec!["rollout-a.jsonl".to_string(), "notes.txt".to_string()],
        );
        directories.insert(PathBuf::from("/h/.codex/sessions/2026/10/15"), vec!["rollout-b.jsonl".to_string()]);
        let files = FakeCodexFiles { directories, asked: RefCell::new(Vec::new()) };
        let found = list_codex_sessions(Path::new("/h/.codex"), started, &files);
        assert_eq!(found.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), [CODEX_ID, CODEX_ID_2]);
        assert_eq!(files.asked.borrow().len(), 2, "a missing day folder is skipped and no other day is read");
    }

    #[test]
    fn test_c_resume_8_elapsed_time_of_ps_becomes_seconds() {
        assert_eq!(elapsed_seconds("05:07"), Some(307));
        assert_eq!(elapsed_seconds("  01:02:03\n"), Some(3723));
        assert_eq!(elapsed_seconds("2-00:00:01"), Some(172_801));
        assert_eq!(elapsed_seconds("abc"), None);
        assert_eq!(elapsed_seconds(""), None);
    }

    #[test]
    fn test_c_resume_9_claude_is_found_by_its_record_codex_by_start_time_and_folder_and_others_are_none() {
        let read = files_of(&[("/home/me/.claude/sessions/42.json", claude_record(&[]))]);
        let started = millis("2026-10-14T01:00:00Z");
        let day = Local.timestamp_millis_opt(started as i64).unwrap();
        let directory =
            PathBuf::from(format!("/home/me/.codex/sessions/{:04}/{:02}/{:02}", day.year(), day.month(), day.day()));
        let mut directories = BTreeMap::new();
        directories.insert(directory, vec!["rollout-a.jsonl".to_string()]);
        let codex = FakeCodexFiles { directories, asked: RefCell::new(Vec::new()) };
        let files = AgentFiles { home_directory: Path::new("/home/me"), read_file: &read, codex: &codex };
        let foreground = |program: &'static str, pid: u32, started_at: Option<u64>| Foreground {
            program,
            pid,
            home: None,
            claude_config_dir: None,
            codex_home: None,
            cwd: "/work/app",
            started_at,
        };
        let claude = resumable_agent(&foreground("claude", 42, None), &files).unwrap();
        assert_eq!(
            (claude.provider.as_str(), claude.session_id.as_str(), claude.config_dir),
            ("claude", CLAUDE_ID, None)
        );
        assert_eq!(resumable_agent(&foreground("claude", 43, None), &files), None);
        let codex_found = resumable_agent(&foreground("codex", 9, Some(started)), &files).unwrap();
        assert_eq!((codex_found.provider.as_str(), codex_found.session_id.as_str()), ("codex", CODEX_ID));
        // 시작 시각을 모르면 Codex 대화를 고르지 않는다.
        assert_eq!(resumable_agent(&foreground("codex", 9, None), &files), None);
        assert_eq!(resumable_agent(&foreground("bash", 9, Some(started)), &files), None);
    }
}
