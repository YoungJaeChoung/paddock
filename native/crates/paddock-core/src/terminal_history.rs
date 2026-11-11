//! 터미널의 마지막 화면 출력과 실행 정보를 파일에 남기는 규칙: 남길 한도, 저장 시점, 기록 모양, 오래된 기록 판정.
//!
//! 앱이 터미널 프로세스째 사라진 뒤(데몬이 끝나거나 컴퓨터가 다시 켜진 뒤) 그 터미널을 되살리는 데 쓴다. 데몬이 쓰고 앱이 읽는다.
//! 파일 읽기·쓰기와 시계는 데몬이 맡고, 이 모듈은 값만 정한다.
use serde::{Deserialize, Serialize};

use crate::account_launch::Launch;
use crate::agent_resume::AgentConversation;

/// 기록의 한도와 시점.
pub struct HistoryLimits;

impl HistoryLimits {
    /// 출력이 멎은 뒤 저장하기까지 기다리는 시간(ms).
    pub const QUIET_MS: u64 = 2000;
    /// 출력이 계속돼도 이 시간(ms) 안에는 한 번 저장한다.
    pub const MAX_WAIT_MS: u64 = 10_000;
    /// 화면 출력으로 남기는 마지막 부분의 최대 바이트.
    pub const N_TAIL_BYTES: usize = 256 * 1024;
    /// 이 기간(ms)이 지난 저장분은 되살릴 터미널이 아니라 버려진 기록이다.
    pub const EXPIRE_MS: u64 = 14 * 24 * 60 * 60 * 1000;
    /// 저장하는 환경 변수. 계정 폴더와 WSL 표지만 둔다 — 로그인 키 같은 값은 파일에 남기지 않는다.
    pub const ENVIRONMENT: [&'static str; 4] = ["CLAUDE_CONFIG_DIR", "CODEX_HOME", "WSLENV", "PADDOCK_TERMINAL"];
    /// 대화로 알아볼 수 있는 에이전트. 앞쪽 프로그램이 이 이름이면 기록의 `foreground`에 남는다.
    pub const AGENTS: [&'static str; 2] = ["claude", "codex"];
}

/// 출력 바이트의 마지막 `max_bytes`바이트를 글자로 돌려준다. 한도를 넘으면 글자 중간이 아닌 글자 경계에서 시작하고,
/// 줄 처음이 앞쪽 1 KiB 안에 있으면 그 줄 처음부터 남긴다.
///
/// Examples
/// --------
/// | `data`                          | `max_bytes` | 결과            |
/// |---------------------------------|-------------|-----------------|
/// | `ab` + `cd`                     | 100         | `abcd`          |
/// | `aaa\nbbb\nccc`                 | 8           | `bbb\nccc`      |
/// | `가나다`(9바이트)               | 4           | `다`            |
/// | `x`를 10번 (줄 경계 없음)       | 4           | `xxxx`          |
pub fn output_tail(
    data: &[u8],
    max_bytes: usize,
) -> String {
    let is_cut = data.len() > max_bytes;
    // 글자 중간(이어지는 바이트)에서 시작하지 않게 글자 경계까지 민다.
    let kept: Vec<u8> = if is_cut {
        data.iter().skip(data.len() - max_bytes).skip_while(|byte| (**byte & 0xc0) == 0x80).copied().collect()
    } else {
        data.to_vec()
    };
    let text = String::from_utf8_lossy(&kept).into_owned();
    match text.split_once('\n') {
        // 줄 처음부터 남긴다. 줄 경계가 앞쪽 1 KiB 안에 있을 때만 쓴다.
        Some((first_line, rest)) if is_cut && first_line.len() < 1024 => rest.to_string(),
        _ => text,
    }
}

/// 저장할 환경 변수만 골라 낸다. 이름이 허용 목록(`HistoryLimits::ENVIRONMENT`)에 없으면 버린다.
///
/// Examples
/// --------
/// | 입력                                           | 결과                  |
/// |------------------------------------------------|-----------------------|
/// | `CODEX_HOME=/c`, `ANTHROPIC_API_KEY=k`         | `CODEX_HOME=/c`       |
/// | (빈 목록)                                      | (빈 목록)             |
pub fn storable_environment(env: &[(String, String)]) -> Vec<(String, String)> {
    env.iter().filter(|(name, _)| HistoryLimits::ENVIRONMENT.contains(&name.as_str())).cloned().collect()
}

/// 저장 시점 판정기. 출력이 올 때마다 `touch`하고, 주기적으로 `is_due`를 물어 저장할 때가 됐는지 본다.
///
/// 마지막 신호로부터 `quiet_ms`가 지나거나 첫 신호로부터 `max_wait_ms`가 지나면 저장할 때다. 저장하면 `finish`로 기다림을 비운다.
///
/// Examples
/// --------
/// | 신호 시각(s)               | quiet 2 · max 10 | 저장 시각(s) |
/// |----------------------------|------------------|--------------|
/// | 0                          |                  | 2            |
/// | 0, 1, 2, … 매초 계속       |                  | 10           |
/// | 0, 5                       |                  | 2, 7         |
#[derive(Clone, Debug)]
pub struct TrailingDebounce {
    quiet_ms: u64,
    max_wait_ms: u64,
    /// 저장을 기다리는 첫 신호의 시각. 기다리는 신호가 없으면 None.
    pending_since: Option<u64>,
    last_touch: u64,
}

impl TrailingDebounce {
    pub fn new(
        quiet_ms: u64,
        max_wait_ms: u64,
    ) -> Self {
        Self { quiet_ms, max_wait_ms, pending_since: None, last_touch: 0 }
    }

    /// 신호(출력)가 왔다.
    pub fn touch(
        &mut self,
        now_ms: u64,
    ) {
        if self.pending_since.is_none() {
            self.pending_since = Some(now_ms);
        }
        self.last_touch = now_ms;
    }

    /// 지금 저장할 때인가.
    pub fn is_due(
        &self,
        now_ms: u64,
    ) -> bool {
        self.pending_since
            .is_some_and(|since| now_ms >= self.last_touch + self.quiet_ms || now_ms >= since + self.max_wait_ms)
    }

    /// 저장을 마쳤다. 기다림을 비운다.
    pub fn finish(&mut self) {
        self.pending_since = None;
    }
}

/// 저장 파일 하나(터미널 하나)의 내용.
///
/// 계정 터미널이면 `launch`(실행 명세와 계정)가 있고, 일반 터미널이면 `shell`·`wsl_distribution`으로 다시 연다.
/// `cwd_start`는 터미널을 처음 연 폴더, `cwd_now`는 저장할 때 셸이 있던 폴더다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub version: u32,
    /// 저장한 시각(ms, 1970년 기준).
    pub saved_at: u64,
    pub shell: String,
    #[serde(default)]
    pub wsl_distribution: Option<String>,
    #[serde(default)]
    pub launch: Option<Launch>,
    #[serde(default)]
    pub cwd_start: Option<String>,
    pub cwd_now: String,
    /// 앞쪽에서 실행 중이던 에이전트 이름(`claude`·`codex`). 아니면 None.
    #[serde(default)]
    pub foreground: Option<String>,
    #[serde(default)]
    pub agent: Option<AgentConversation>,
    /// 화면 출력의 마지막 부분.
    pub output: String,
}

impl HistoryRecord {
    /// 현재 파일 형식 번호.
    pub const VERSION: u32 = 1;
}

/// 기록을 만들 재료.
pub struct HistoryInput<'a> {
    pub saved_at: u64,
    pub shell: &'a str,
    pub wsl_distribution: Option<&'a str>,
    pub launch: Option<&'a Launch>,
    pub cwd_start: Option<&'a str>,
    /// 저장할 때의 현재 폴더(운영체제에서 읽은 값, 없으면 셸이 알린 값). 모르면 None.
    pub cwd_now: Option<&'a str>,
    /// 앞쪽 프로그램 이름.
    pub program: Option<&'a str>,
    pub agent: Option<&'a AgentConversation>,
    pub output: &'a [u8],
}

/// 재료에서 저장할 기록을 만든다.
///
/// 현재 폴더를 모르면 처음 폴더를 쓰고, 그것도 없으면 빈 글자다. 계정 터미널의 환경 변수는 허용 목록만 남긴다 — 로그인 키 같은 값이 파일에 들어가지 않게 한다.
/// 앞쪽 프로그램이 claude·codex가 아니면 `foreground`는 None이다.
pub fn build_record(input: &HistoryInput) -> HistoryRecord {
    let launch = input.launch.map(|launch| Launch { env: storable_environment(&launch.env), ..launch.clone() });
    let cwd_now = input.cwd_now.filter(|cwd| !cwd.is_empty()).or(input.cwd_start).unwrap_or("");
    HistoryRecord {
        version: HistoryRecord::VERSION,
        saved_at: input.saved_at,
        shell: input.shell.to_string(),
        wsl_distribution: input.wsl_distribution.map(str::to_string),
        launch,
        cwd_start: input.cwd_start.map(str::to_string),
        cwd_now: cwd_now.to_string(),
        foreground: input.program.filter(|program| HistoryLimits::AGENTS.contains(program)).map(str::to_string),
        agent: input.agent.cloned(),
        output: output_tail(input.output, HistoryLimits::N_TAIL_BYTES),
    }
}

/// 저장 파일 내용을 읽는다. 깨졌거나 모르는 형식 번호면 None이다.
pub fn parse_record(text: &str) -> Option<HistoryRecord> {
    serde_json::from_str::<HistoryRecord>(text).ok().filter(|record| record.version == HistoryRecord::VERSION)
}

/// 이 기록이 `age_ms` 동안 갱신되지 않았다면 버려진 기록인가.
pub fn is_expired(age_ms: u64) -> bool {
    age_ms > HistoryLimits::EXPIRE_MS
}

/// 데몬 식별 글자(`pid@시작 시각`)를 폴더 이름으로 쓸 수 있는 글자로 바꾼다. 영문자·숫자·`@`·`.`·`_`·`-` 밖의 글자(`:`·`+` 등)는 `-`로 바꾼다.
///
/// Examples
/// --------
/// | 식별 글자                                  | 폴더 이름                                  |
/// |--------------------------------------------|--------------------------------------------|
/// | `412@2026-10-16T21:00:00.5+09:00`          | `412@2026-10-16T21-00-00.5-09-00`          |
pub fn directory_name(identity: &str) -> String {
    identity
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() || "@._-".contains(character) { character } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
    }

    #[test]
    fn test_c_history_1_the_tail_starts_at_a_line_start_or_a_character_boundary() {
        assert_eq!(output_tail(b"abcd", 100), "abcd");
        assert_eq!(output_tail(b"aaa\nbbb\nccc", 8), "bbb\nccc");
        assert_eq!(output_tail("가나다".as_bytes(), 4), "다");
        assert_eq!(output_tail(&[b'x'; 10], 4), "xxxx");
    }

    #[test]
    fn test_c_history_2_a_tail_of_huge_output_keeps_only_the_last_limit() {
        let mut all = String::new();
        for line in 0..50_000 {
            all.push_str(&format!("line {line}\n"));
        }
        let text = output_tail(all.as_bytes(), HistoryLimits::N_TAIL_BYTES);
        assert!(text.len() <= HistoryLimits::N_TAIL_BYTES);
        assert!(text.ends_with("line 49999\n"));
        assert!(
            text.starts_with("line "),
            "the tail starts at a line start: {:?}",
            text.chars().take(12).collect::<String>()
        );
    }

    #[test]
    fn test_c_history_3_the_current_folder_falls_back_to_the_start_folder_and_then_to_nothing() {
        let record = |cwd_start: Option<&str>, cwd_now: Option<&str>| {
            build_record(&HistoryInput {
                saved_at: 1,
                shell: "bash",
                wsl_distribution: None,
                launch: None,
                cwd_start,
                cwd_now,
                program: None,
                agent: None,
                output: b"",
            })
            .cwd_now
        };
        assert_eq!(record(Some("/work/app"), Some("/work/app/sub")), "/work/app/sub");
        assert_eq!(record(Some("/work/app"), None), "/work/app");
        assert_eq!(record(None, None), "");
    }

    #[test]
    fn test_c_history_4_only_allowed_environment_names_are_stored() {
        let stored = storable_environment(&env(&[
            ("CODEX_HOME", "/c"),
            ("ANTHROPIC_API_KEY", "k"),
            ("PADDOCK_TERMINAL", "t"),
            ("WSLENV", "w"),
        ]));
        assert_eq!(stored, env(&[("CODEX_HOME", "/c"), ("PADDOCK_TERMINAL", "t"), ("WSLENV", "w")]));
        assert!(storable_environment(&[]).is_empty());
    }

    /// 신호 시각(초) 목록에 따라 저장이 일어나는 시각(초)을 100 ms 간격으로 훑어 구한다.
    fn saved_at_seconds(touches: &[u64]) -> Vec<u64> {
        let mut debounce = TrailingDebounce::new(HistoryLimits::QUIET_MS, HistoryLimits::MAX_WAIT_MS);
        let mut saved = Vec::new();
        for now_ms in (0..=30_000).step_by(100) {
            if touches.iter().any(|touch| touch * 1000 == now_ms) {
                debounce.touch(now_ms);
            }
            if debounce.is_due(now_ms) {
                saved.push(now_ms / 1000);
                debounce.finish();
            }
        }
        saved
    }

    #[test]
    fn test_c_history_5_output_saves_two_seconds_after_it_stops_and_within_ten_seconds_while_it_continues() {
        assert_eq!(saved_at_seconds(&[0]), [2]);
        assert_eq!(saved_at_seconds(&[0, 5]), [2, 7]);
        let every_second: Vec<u64> = (0..=24).collect();
        assert_eq!(saved_at_seconds(&every_second)[0], 10);
        assert!(saved_at_seconds(&[]).is_empty(), "nothing to save without output");
    }

    #[test]
    fn test_c_history_6_a_record_has_the_shell_folders_launch_and_no_login_values() {
        let launch = Launch {
            program: "powershell.exe".into(),
            args: vec!["-NoLogo".into()],
            env: env(&[("CLAUDE_CONFIG_DIR", "/acc"), ("ANTHROPIC_API_KEY", "secret")]),
            env_remove: vec!["OPENAI_API_KEY".into()],
            cwd: Some("/work/app".into()),
            title: Some("Claude \u{b7} Main".into()),
            account: None,
        };
        let agent = AgentConversation {
            provider: "claude".into(),
            session_id: "s".into(),
            cwd: "/work/app/sub".into(),
            config_dir: None,
        };
        let record = build_record(&HistoryInput {
            saved_at: 77,
            shell: "powershell",
            wsl_distribution: None,
            launch: Some(&launch),
            cwd_start: Some("/work/app"),
            cwd_now: Some("/work/app/sub"),
            program: Some("claude"),
            agent: Some(&agent),
            output: b"screen",
        });
        assert_eq!((record.version, record.saved_at, record.shell.as_str()), (1, 77, "powershell"));
        assert_eq!((record.cwd_start.as_deref(), record.cwd_now.as_str()), (Some("/work/app"), "/work/app/sub"));
        assert_eq!(record.foreground.as_deref(), Some("claude"));
        assert_eq!(record.agent, Some(agent));
        assert_eq!(record.output, "screen");
        let stored = record.launch.as_ref().expect("the launch is kept");
        assert_eq!(stored.env, env(&[("CLAUDE_CONFIG_DIR", "/acc")]));
        assert_eq!(stored.env_remove, launch.env_remove, "names to remove are not values");
        assert!(!serde_json::to_string(&record).unwrap().contains("secret"));
        // 앞쪽 프로그램이 에이전트가 아니면 foreground는 비어 있다.
        let shell_only =
            build_record(&HistoryInput { program: Some("bash"), launch: None, agent: None, ..input_of(&record) });
        assert_eq!(shell_only.foreground, None);
    }

    fn input_of(record: &HistoryRecord) -> HistoryInput<'_> {
        HistoryInput {
            saved_at: record.saved_at,
            shell: &record.shell,
            wsl_distribution: None,
            launch: None,
            cwd_start: None,
            cwd_now: None,
            program: None,
            agent: None,
            output: b"",
        }
    }

    #[test]
    fn test_c_history_7_a_record_roundtrips_and_a_broken_or_unknown_one_reads_as_nothing() {
        let record = build_record(&HistoryInput { output: "한글 화면".as_bytes(), ..input_of(&sample_record()) });
        let text = serde_json::to_string(&record).unwrap();
        assert_eq!(parse_record(&text), Some(record));
        assert_eq!(parse_record("{broken"), None);
        assert_eq!(parse_record(&text.replace("\"version\":1", "\"version\":2")), None);
        assert_eq!(parse_record(""), None);
    }

    fn sample_record() -> HistoryRecord {
        HistoryRecord {
            version: 1,
            saved_at: 5,
            shell: "bash".into(),
            wsl_distribution: None,
            launch: None,
            cwd_start: None,
            cwd_now: String::new(),
            foreground: None,
            agent: None,
            output: String::new(),
        }
    }

    #[test]
    fn test_c_history_8_a_record_not_updated_for_more_than_fourteen_days_is_abandoned() {
        let day = 24 * 60 * 60 * 1000;
        assert!(!is_expired(13 * day));
        assert!(!is_expired(14 * day));
        assert!(is_expired(14 * day + 1));
    }

    #[test]
    fn test_c_history_9_a_daemon_identity_becomes_a_folder_name_without_characters_windows_refuses() {
        assert_eq!(directory_name("412@2026-10-16T21:00:00.5+09:00"), "412@2026-10-16T21-00-00.5-09-00");
        assert_eq!(directory_name("7@t0"), "7@t0");
        assert!(!directory_name("a:b/c\\d*e").contains([':', '/', '\\', '*']));
    }
}
