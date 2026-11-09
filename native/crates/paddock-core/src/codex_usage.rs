//! Codex 공식 CLI에게 계정 한도를 묻는 규칙: 어떤 프로그램을 어떤 인자·환경으로 띄울지, 주고받는 JSON-RPC 글, 응답에서 창 읽기,
//! 여러 실행 환경의 읽기 결과 가운데 믿을 값 고르기.
//!
//! 인증은 Codex 안에서만 일어난다. 이 모듈은 `initialize`·`account/rateLimits/read` 두 메시지만 주고받고 대화나 모델 요청을 만들지 않는다.
//! 프로세스 실행과 시간 제한은 호출자(앱의 작업 스레드)가 맡는다.
use std::time::Duration;

use serde_json::{Value, json};

use crate::account_launch::AUTH_ENV;
use crate::agent_account::Runtime;
use crate::cwd_report::encode_powershell;
use crate::usage_model::{Reading, Reset, UsageWindow, has_current_window, window_label};

/// 실시간 조회의 한도.
pub struct CodexUsage;

impl CodexUsage {
    /// 성공한 읽기를 계정 폴더마다 다시 쓰는 시간(초).
    pub const CACHE_SECONDS: u64 = 120;
    /// 조회 하나가 응답을 기다리는 최대 시간.
    pub const TIMEOUT: Duration = Duration::from_secs(15);
    /// 입력을 닫은 뒤 CLI가 스스로 끝나기를 기다리는 시간. 이 안에 안 끝나면 이 조회가 만든 프로세스만 강제로 끝낸다.
    pub const CLEANUP: Duration = Duration::from_secs(1);
    /// 표준 출력이 이 크기를 넘으면 조회를 실패로 끝낸다.
    pub const N_OUTPUT_BYTES: usize = 1024 * 1024;
    pub const UNAVAILABLE: &'static str =
        "Codex usage could not be read. Open this account in Codex, sign in if needed, and try again.";
    pub const TIMED_OUT: &'static str = "Codex usage took too long to respond. Try again.";
    pub const READ_FAILED: &'static str =
        "Codex usage could not be read. Open Codex, sign in if needed, and try again.";
    pub const AMBIGUOUS: &'static str =
        "More than one CLI environment has usage. Choose a saved account to see its usage.";
}

/// 조회가 실패했다(시작 못 함·시간 초과·응답 오류). 이유 문장은 담지 않는다 — CLI의 원문 오류가 밖으로 새지 않게 한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LookupFailed;

/// 한도를 물을 Codex 설정 폴더 하나와 그 환경.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexEnv {
    pub runtime: Runtime,
    /// WSL이면 배포판 이름(비면 기본 배포판).
    pub wsl_distribution: Option<String>,
    /// 실행 환경 안에서 보인 설정 폴더(`CODEX_HOME`). WSL이면 Linux 경로다.
    pub config_dir: String,
    /// 이 앱이 읽을 수 있는 세션 기록 폴더(WSL은 `\\wsl.localhost\…` 경로).
    pub sessions_dir: String,
}

impl CodexEnv {
    /// 읽기 캐시의 키: 실행 환경·배포판·설정 폴더.
    pub fn cache_key(&self) -> String {
        format!(
            "{}|{}|{}",
            if self.runtime == Runtime::Wsl { "wsl" } else { "native" },
            self.wsl_distribution.as_deref().unwrap_or(""),
            self.config_dir
        )
    }
}

/// 띄울 프로세스.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    /// 설정할 환경 변수.
    pub env_set: Vec<(String, String)>,
    /// 지울 환경 변수(로그인을 우회할 수 있는 변수들).
    pub env_remove: Vec<String>,
    /// 자기 프로세스 그룹으로 띄운다(끝낼 때 그룹 전체를 끝낸다). Linux·macOS 이 컴퓨터 쪽이다.
    pub own_process_group: bool,
    /// 끝낼 때 `taskkill /T`로 이 조회가 만든 프로세스 트리를 끝낸다(Windows 이 컴퓨터 쪽: npm 래퍼가 자식을 만든다).
    pub kill_tree: bool,
}

/// 저장한 환경(설정 폴더)에서 `codex app-server`를 띄우는 명령. 셸 초기화 뒤의 환경을 쓰되, 계정 경로를 셸 글자에 끼우지 않는다.
///
/// WSL은 `wsl.exe`가 Linux 자식에게 신호를 안정적으로 못 보내므로 `timeout`이 이 조회의 프로세스 그룹만 14초에 끝내게 한다.
///
/// Examples
/// --------
/// | 환경              | 호스트  | 프로그램                                           |
/// |-------------------|---------|----------------------------------------------------|
/// | 이 컴퓨터         | Linux   | `/bin/bash -ic <script> paddock-usage <폴더>`      |
/// | WSL(Debian Saved) | Windows | `wsl.exe --distribution "Debian Saved" --exec /bin/bash -ic …` |
/// | 이 컴퓨터         | Windows | `powershell.exe … -EncodedCommand <codex app-server>` |
pub fn codex_usage_command(
    env: &CodexEnv,
    host_is_windows: bool,
) -> Invocation {
    let env_remove: Vec<String> = AUTH_ENV.iter().map(ToString::to_string).collect();
    let env_set = vec![("CODEX_HOME".to_string(), env.config_dir.clone())];
    let is_wsl = env.runtime == Runtime::Wsl;
    if is_wsl || !host_is_windows {
        let launcher = if is_wsl { "timeout --signal=TERM --kill-after=2s 14s " } else { "" };
        let script = format!(
            "unset {}; export CODEX_HOME=\"$1\"; cd -- \"$CODEX_HOME\" || exit 1; exec {launcher}codex app-server",
            AUTH_ENV.join(" ")
        );
        let mut args = vec!["-ic".to_string(), script, "paddock-usage".to_string(), env.config_dir.clone()];
        let program;
        let cwd;
        if is_wsl {
            program = "wsl.exe".to_string();
            let mut wsl_args: Vec<String> = Vec::new();
            if let Some(distribution) = env.wsl_distribution.as_deref().filter(|name| !name.is_empty()) {
                wsl_args.extend(["--distribution".to_string(), distribution.to_string()]);
            }
            wsl_args.extend(["--exec".to_string(), "/bin/bash".to_string()]);
            wsl_args.append(&mut args);
            args = wsl_args;
            cwd = None;
        } else {
            program = "/bin/bash".to_string();
            cwd = Some(env.config_dir.clone());
        }
        Invocation { program, args, cwd, env_set, env_remove, own_process_group: !is_wsl, kill_tree: false }
    } else {
        let script = "$ErrorActionPreference = \"Stop\"; $cli = Get-Command codex -CommandType Application -ErrorAction Stop | Select-Object -First 1; & $cli.Source app-server";
        Invocation {
            program: "powershell.exe".to_string(),
            args: ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand"]
                .into_iter()
                .map(str::to_string)
                .chain(std::iter::once(encode_powershell(script)))
                .collect(),
            cwd: Some(env.config_dir.clone()),
            env_set,
            env_remove,
            own_process_group: false,
            kill_tree: true,
        }
    }
}

/// `initialize` 요청 한 줄.
pub fn initialize_request() -> String {
    json!({"method": "initialize", "id": 1, "params": {"clientInfo": {"name": "paddock_usage", "title": "Paddock", "version": "0.1.0"}}})
        .to_string()
}

/// `initialize`에 답이 오면 이어 보내는 두 줄: `initialized` 알림과 한도 읽기 요청.
pub fn after_initialize_requests() -> [String; 2] {
    [
        json!({"method": "initialized", "params": {}}).to_string(),
        json!({"method": "account/rateLimits/read", "id": 2}).to_string(),
    ]
}

/// 조회 대화의 한 걸음 결과.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// 아직 기다린다.
    Wait,
    /// 이 줄들을 보낸다.
    Send(Vec<String>),
    /// 끝났다. 한도 읽기 응답(`result`)이거나 실패다. 실패 사유는 담지 않는다 — CLI의 원문 오류가 밖으로 새지 않게 한다.
    Done(Result<Value, LookupFailed>),
}

/// 표준 출력 줄을 받아 대화를 진행하는 기계: `initialize`(id 1) 응답 → `initialized` + 한도 읽기(id 2) → 응답.
#[derive(Debug, Default)]
pub struct Conversation {
    initialized: bool,
}

impl Conversation {
    /// 출력 한 줄을 처리한다. JSON이 아니거나 모르는 id의 줄은 건너뛴다.
    pub fn on_line(
        &mut self,
        line: &str,
    ) -> Step {
        let message = serde_json::from_str::<Value>(line).ok();
        let id = message.as_ref().and_then(|message| message.get("id")).and_then(Value::as_i64);
        let usable = message.as_ref().is_some_and(|message| {
            message.get("error").is_none_or(Value::is_null)
                && message.get("result").is_some_and(|result| !result.is_null())
        });
        match (id, self.initialized) {
            (Some(1), false) => {
                if usable {
                    self.initialized = true;
                    Step::Send(after_initialize_requests().to_vec())
                } else {
                    Step::Done(Err(LookupFailed))
                }
            }
            (Some(2), true) => {
                if usable {
                    Step::Done(Ok(message.and_then(|message| message.get("result").cloned()).unwrap_or(Value::Null)))
                } else {
                    Step::Done(Err(LookupFailed))
                }
            }
            _ => Step::Wait,
        }
    }
}

/// 공식 한도 응답에서 기본 묶음의 창을 읽는다. 모델별 묶음은 계정 합계에 합치지 않는다.
///
/// Examples
/// --------
/// | 응답                                                         | 결과                              |
/// |--------------------------------------------------------------|-----------------------------------|
/// | primary 0%(300분)·secondary 24.6%(10080분)                   | `5h 0%`, `week 25%`               |
/// | primary 83%(10080분)·secondary null                          | `week 83%` (5h 창을 만들지 않음)  |
/// | primary 14%(60분), 초기화 시각 null                          | `1h 14%`, 시각 모름               |
/// | usedPercent가 글자                                           | (빈 목록)                         |
pub fn rate_limit_windows(response: &Value) -> Vec<UsageWindow> {
    let limits = response.get("rateLimits");
    ["primary", "secondary"]
        .iter()
        .filter_map(|name| {
            let entry = limits?.get(name)?;
            let used = entry.get("usedPercent").and_then(Value::as_f64).filter(|value| value.is_finite())?;
            let minutes = entry
                .get("windowDurationMins")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value > 0.0)?;
            let reset = match entry.get("resetsAt").and_then(Value::as_f64).filter(|value| value.is_finite()) {
                Some(seconds) => Reset::At(seconds as i64),
                None => Reset::Unknown,
            };
            Some(UsageWindow {
                label: window_label(minutes as i64),
                used: used.clamp(0.0, 100.0).round() as i64,
                reset,
            })
        })
        .collect()
}

/// 기본 환경(이 컴퓨터·WSL)마다 읽은 결과 가운데 믿을 값 하나를 고른다.
///
/// 값은 믿을 수 있는 차례로 고른다 — 실시간 값 → 초기화 전 창이 남은 세션 기록 → 초기화가 모두 지난 세션 기록.
/// 앞 차례에 값이 있으면 뒤 차례는 세지 않는다. 그래서 한 환경의 토큰이 무효라 며칠 전 기록만 남아도 다른 환경의 실시간 값을 가리지 않는다.
/// 같은 차례의 값이 둘 이상이고 창 사용량이 서로 다르면 어느 계정의 값인지 단정할 수 없어 오류다.
///
/// Examples
/// --------
/// | 이 컴퓨터                | WSL                                | 결과                  |
/// |--------------------------|------------------------------------|-----------------------|
/// | 실시간 실패 + 9일 전 기록 | 실시간 0%                          | WSL 실시간 0%         |
/// | 실시간 20%               | 실시간 80%                         | 오류(more than one)   |
/// | 실시간 실패 + 기록 없음  | 실시간 실패 + 어제 기록(초기화 전) | 어제 기록(recorded)   |
pub fn pick_reading(
    readings: &[Reading],
    now_seconds: i64,
) -> Reading {
    let with_windows: Vec<&Reading> = readings.iter().filter(|reading| !reading.windows.is_empty()).collect();
    let tiers: [Vec<&Reading>; 3] = [
        with_windows.iter().copied().filter(|reading| !reading.recorded).collect(),
        with_windows
            .iter()
            .copied()
            .filter(|reading| reading.recorded && has_current_window(&reading.windows, now_seconds))
            .collect(),
        with_windows
            .iter()
            .copied()
            .filter(|reading| reading.recorded && !has_current_window(&reading.windows, now_seconds))
            .collect(),
    ];
    let chosen: Vec<&Reading> = tiers.into_iter().find(|tier| !tier.is_empty()).unwrap_or_default();
    // 같은 계정의 같은 한도라면 창별 사용량이 같다. 다르면 서로 다른 계정이다.
    let shape = |reading: &Reading| -> Vec<(String, i64)> {
        reading.windows.iter().map(|window| (window.label.clone(), window.used)).collect()
    };
    let first = chosen.first().copied();
    if first.is_some_and(|first| chosen.iter().any(|reading| shape(reading) != shape(first))) {
        Reading { error: Some(CodexUsage::AMBIGUOUS.to_string()), ..Reading::default() }
    } else {
        first.cloned().or_else(|| readings.iter().find(|reading| reading.error.is_some()).cloned()).unwrap_or_default()
    }
}

/// 환경 하나의 읽기: 실시간 조회 → 실패하거나 창이 없으면 세션 기록.
///
/// 읽지 못했고 기록도 없으면 `READ_FAILED` 문장을 담은 읽기 결과다(원문 오류는 담지 않는다).
pub fn read_environment(
    env: &CodexEnv,
    live: &mut dyn FnMut(&CodexEnv) -> Result<Reading, LookupFailed>,
    recorded: &mut dyn FnMut(&CodexEnv) -> Result<Reading, LookupFailed>,
) -> Reading {
    let mut reading = Reading::default();
    let mut failed = false;
    match live(env) {
        Ok(value) => reading = value,
        Err(LookupFailed) => failed = true,
    }
    if reading.windows.is_empty() {
        match recorded(env) {
            Ok(value) if !value.windows.is_empty() => reading = Reading { recorded: true, ..value },
            Ok(_) => {}
            Err(LookupFailed) => failed = true,
        }
    }
    if reading.windows.is_empty() && failed {
        reading.error = Some(CodexUsage::READ_FAILED.to_string());
    }
    reading
}

/// 기본 환경들의 읽기 결과 하나. 한 환경이 실패해도 다른 도구·환경을 가리지 않는다.
pub fn read_available(
    envs: &[CodexEnv],
    now_seconds: i64,
    live: &mut dyn FnMut(&CodexEnv) -> Result<Reading, LookupFailed>,
    recorded: &mut dyn FnMut(&CodexEnv) -> Result<Reading, LookupFailed>,
) -> Reading {
    let readings: Vec<Reading> = envs.iter().map(|env| read_environment(env, live, recorded)).collect();
    pick_reading(&readings, now_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> Value {
        json!({
            "rateLimitsByLimitId": {"codex": {"primary": {"usedPercent": 0, "windowDurationMins": 300, "resetsAt": 1999999999}}, "other": {"primary": {"usedPercent": 99, "windowDurationMins": 300}}},
            "rateLimits": {"primary": {"usedPercent": 0, "windowDurationMins": 300, "resetsAt": 1999999999}, "secondary": {"usedPercent": 24.6, "windowDurationMins": 10080, "resetsAt": 1999999998}}
        })
    }

    fn env(runtime: Runtime) -> CodexEnv {
        CodexEnv {
            runtime,
            wsl_distribution: (runtime == Runtime::Wsl).then(|| "Debian Saved".to_string()),
            config_dir: if runtime == Runtime::Wsl {
                "/home/person's data/codex".to_string()
            } else {
                "/profiles/fixture/codex".to_string()
            },
            sessions_dir: "/not-created".to_string(),
        }
    }

    fn live_reading(
        label: &str,
        used: i64,
        reset: i64,
        updated_at: i64,
    ) -> Reading {
        Reading {
            windows: vec![UsageWindow { label: label.to_string(), used, reset: Reset::At(reset) }],
            updated_at: Some(updated_at),
            ..Reading::default()
        }
    }

    fn windows(entries: &[(&str, i64, Reset)]) -> Vec<UsageWindow> {
        entries
            .iter()
            .map(|(label, used, reset)| UsageWindow { label: label.to_string(), used: *used, reset: *reset })
            .collect()
    }

    #[test]
    fn test_cu01_official_usage_chooses_the_cli_default_bucket_preserves_zero_and_returns_only_known_numeric_fields() {
        assert_eq!(
            rate_limit_windows(&limits()),
            windows(&[("5h", 0, Reset::At(1999999999)), ("week", 25, Reset::At(1999999998))])
        );
        assert_eq!(
            rate_limit_windows(&json!({"rateLimits": {"primary": {"usedPercent": "bad", "windowDurationMins": 300}}})),
            Vec::new()
        );
        assert_eq!(
            rate_limit_windows(
                &json!({"rateLimits": {"primary": {"usedPercent": 14, "windowDurationMins": 60, "resetsAt": null}}})
            ),
            windows(&[("1h", 14, Reset::Unknown)])
        );
    }

    #[test]
    fn test_cu05_a_weekly_primary_is_shown_as_weekly_and_an_absent_secondary_does_not_create_a_five_hour_limit() {
        let weekly = json!({"rateLimits": {"primary": {"usedPercent": 83, "windowDurationMins": 10080, "resetsAt": 1791690576}, "secondary": null}});
        assert_eq!(rate_limit_windows(&weekly), windows(&[("week", 83, Reset::At(1791690576))]));
        assert_eq!(
            rate_limit_windows(
                &json!({"rateLimits": null, "rateLimitsByLimitId": {"other": limits()["rateLimits"].clone()}})
            ),
            Vec::new()
        );
    }

    #[test]
    fn test_cu10_an_unreported_reset_time_stays_unknown_while_a_confirmed_elapsed_reset_is_displayed_separately() {
        use crate::usage_model::{current_windows, describe};
        let unknown = rate_limit_windows(
            &json!({"rateLimits": {"primary": {"usedPercent": 14, "windowDurationMins": 60, "resetsAt": null}}}),
        );
        assert_eq!(unknown[0].reset, Reset::Unknown);
        assert_eq!(describe(&unknown[0], 200), "14% used \u{b7} 86% left");
        let elapsed = current_windows(
            &rate_limit_windows(
                &json!({"rateLimits": {"primary": {"usedPercent": 14, "windowDurationMins": 60, "resetsAt": 100}}}),
            ),
            200,
        );
        assert_eq!((elapsed[0].reset, elapsed[0].used), (Reset::Done, 0));
        assert!(describe(&elapsed[0], 200).contains("new window"));
    }

    #[test]
    fn test_cu03_the_account_environment_removes_alternate_authentication_selectors_and_wsl_uses_the_saved_distribution()
     {
        let native = codex_usage_command(&env(Runtime::Native), false);
        assert_eq!(native.env_set, vec![("CODEX_HOME".to_string(), "/profiles/fixture/codex".to_string())]);
        for name in ["OPENAI_API_KEY", "CODEX_AUTH_JSON", "ANTHROPIC_API_KEY"] {
            assert!(native.env_remove.iter().any(|removed| removed == name), "{name}");
        }
        assert!(native.own_process_group && native.cwd.as_deref() == Some("/profiles/fixture/codex"));
        let wsl = codex_usage_command(&env(Runtime::Wsl), true);
        assert_eq!(wsl.program, "wsl.exe");
        assert_eq!(wsl.args[..4], ["--distribution", "Debian Saved", "--exec", "/bin/bash"].map(str::to_string));
        assert_eq!(wsl.args.last().map(String::as_str), Some("/home/person's data/codex"));
        let script = &wsl.args[wsl.args.len() - 3];
        assert!(script.contains("export CODEX_HOME=\"$1\""), "{script}");
        assert!(script.contains("unset ANTHROPIC_API_KEY") && script.contains("OPENAI_API_KEY"), "{script}");
        assert!(script.contains("timeout --signal=TERM --kill-after=2s 14s codex app-server"), "{script}");
        assert!(!script.contains("person"), "the account folder never enters the shell text");
        let windows_native = codex_usage_command(&env(Runtime::Native), true);
        assert_eq!(windows_native.program, "powershell.exe");
        assert!(windows_native.kill_tree && !windows_native.own_process_group);
    }

    #[test]
    fn test_cu_the_conversation_sends_initialized_and_the_limit_request_only_after_initialize_succeeds() {
        let mut conversation = Conversation::default();
        assert_eq!(conversation.on_line("not json"), Step::Wait);
        assert_eq!(
            conversation.on_line(r#"{"id":2,"result":{}}"#),
            Step::Wait,
            "a limit answer before initialize is not trusted"
        );
        let Step::Send(lines) = conversation.on_line(r#"{"id":1,"result":{}}"#) else { panic!("send") };
        let methods: Vec<String> = lines
            .iter()
            .map(|line| serde_json::from_str::<Value>(line).unwrap()["method"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(methods, vec!["initialized", "account/rateLimits/read"]);
        let Step::Done(Ok(result)) = conversation.on_line(&json!({"id": 2, "result": limits()}).to_string()) else {
            panic!("done")
        };
        assert_eq!(rate_limit_windows(&result).len(), 2);
    }

    #[test]
    fn test_cu04_official_server_errors_never_expose_raw_cli_output() {
        let mut conversation = Conversation::default();
        let step =
            conversation.on_line(r#"{"id":1,"error":{"code":-32000,"message":"private-value-must-not-escape"}}"#);
        assert_eq!(step, Step::Done(Err(LookupFailed)));
        assert!(!format!("{step:?}").contains("private"));
    }

    #[test]
    fn test_cu08_default_cli_lookup_keeps_a_healthy_environment_when_another_environment_fails() {
        let mut live = |env: &CodexEnv| {
            if env.config_dir == "/missing" { Err(LookupFailed) } else { Ok(live_reading("5h", 0, 1999999999, 100)) }
        };
        let mut recorded = |_: &CodexEnv| Ok(Reading::default());
        let missing = CodexEnv { config_dir: "/missing".to_string(), ..env(Runtime::Native) };
        let value = read_available(&[missing.clone(), env(Runtime::Wsl)], 1000, &mut live, &mut recorded);
        assert_eq!(value.windows[0].used, 0);
        assert_eq!(value.error, None);
        let failed = read_available(&[missing], 1000, &mut live, &mut recorded);
        assert!(failed.windows.is_empty());
        assert!(failed.error.unwrap().contains("could not be read"));
    }

    #[test]
    fn test_cu09_two_default_environments_with_usage_require_account_selection_instead_of_guessing_from_response_times()
    {
        let mut live =
            |env: &CodexEnv| Ok(live_reading("week", if env.runtime == Runtime::Wsl { 80 } else { 20 }, 1999999999, 5));
        let mut recorded = |_: &CodexEnv| Ok(Reading::default());
        let value = read_available(&[env(Runtime::Native), env(Runtime::Wsl)], 1000, &mut live, &mut recorded);
        assert!(value.windows.is_empty());
        assert!(value.error.unwrap().contains("saved account"));
    }

    #[test]
    fn test_cu11_a_live_reading_in_one_environment_wins_over_a_session_record_left_in_another_whose_sign_in_failed() {
        let now = 1_000_000;
        for (reset, modified) in [(now - 3600, now - 85 * 3600), (now + 3600, now - 3600)] {
            let mut live = |env: &CodexEnv| {
                if env.runtime == Runtime::Native {
                    Err(LookupFailed)
                } else {
                    Ok(live_reading("30d", 0, now + 86400, now))
                }
            };
            let mut recorded = |_: &CodexEnv| {
                Ok(Reading {
                    windows: windows(&[("week", 29, Reset::At(reset))]),
                    updated_at: Some(modified),
                    ..Reading::default()
                })
            };
            let value = read_available(&[env(Runtime::Native), env(Runtime::Wsl)], now, &mut live, &mut recorded);
            assert_eq!(value.error, None, "a fallback record is not counted against a live reading");
            assert_eq!(
                value.windows.iter().map(|window| (window.label.as_str(), window.used)).collect::<Vec<_>>(),
                vec![("30d", 0)]
            );
            assert!(!value.recorded);
        }
    }

    #[test]
    fn test_cu12_equal_live_readings_agree_and_a_session_record_is_used_only_without_a_live_reading_and_is_marked_as_recorded()
     {
        let now = 1_000_000;
        let mut live_ok = |_: &CodexEnv| Ok(live_reading("week", 16, now + 600, now));
        let mut none = |_: &CodexEnv| Ok(Reading::default());
        let same = read_available(&[env(Runtime::Native), env(Runtime::Wsl)], now, &mut live_ok, &mut none);
        assert_eq!(same.windows[0].used, 16, "two environments signed in to one account report one value");
        let mut offline = |_: &CodexEnv| Err(LookupFailed);
        let mut records = |env: &CodexEnv| {
            let (used, reset, modified) = if env.runtime == Runtime::Native {
                (29, now - 3600, now - 85 * 3600)
            } else {
                (85, now + 3600, now - 18 * 3600)
            };
            Ok(Reading {
                windows: windows(&[("week", used, Reset::At(reset))]),
                updated_at: Some(modified),
                ..Reading::default()
            })
        };
        let recorded = read_available(&[env(Runtime::Native), env(Runtime::Wsl)], now, &mut offline, &mut records);
        assert!(recorded.recorded);
        assert_eq!(
            recorded.windows[0].used, 85,
            "a record whose windows have all reset does not compete with one that is still current"
        );
        assert_eq!(recorded.updated_at, Some(now - 18 * 3600));
        let only_elapsed = read_available(&[env(Runtime::Native)], now, &mut offline, &mut records);
        assert!(only_elapsed.recorded, "an elapsed record is still shown when nothing else exists, marked as recorded");
    }
}
