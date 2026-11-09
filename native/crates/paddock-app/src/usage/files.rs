//! 사용량 기록 읽기: 계정 폴더의 `paddock-usage/claude.json`, Codex 세션 기록, 기본 CLI의 기록 위치.
//!
//! 모두 읽기만 하고 파일을 만들지 않는다. 대화 내용은 한도 줄 말고는 읽지 않고, 결과에 담지 않는다.
//! 링크로 바뀐 폴더·파일은 거부한다 — 계정 폴더 밖의 파일을 사용량으로 읽지 않게 한다. 파일이 없는 것은 아직 기록이 없다는 뜻이라 오류가 아니다.
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use paddock_core::account_login::{WslMapping, default_config_dir, default_login_file};
use paddock_core::agent_account::{AccountProfile, Runtime, Scope};
use paddock_core::codex_usage::{CodexEnv, LookupFailed, read_available};
use paddock_core::usage_model::{ClaudeState, Reading, claude_windows, codex_windows};
use paddock_core::usage_scopes::Snapshot;
use paddock_protocol::shared_config::config_dir;
use serde_json::Value;

use super::codex::CodexReader;
use super::login::{claude_default_logins, codex_default_logins, link_of};
use crate::accounts::store::read_profiles;

/// Codex 세션 기록 끝부분만 읽는다. 마지막 한도 기록은 파일 끝 가까이에 있다.
pub const N_TAIL_BYTES: u64 = 512 * 1024;

/// 최근 이 수의 날짜 폴더, 이 수의 파일까지만 훑는다.
const N_RECENT_DAYS: usize = 2;
const N_RECENT_FILES: usize = 50;

/// 사용량 파일을 읽을 수 없다(링크·종류 불일치·권한·깨진 JSON). 이유 문장은 담지 않는다 — 화면은 `Unavailable`과 다시 시도만 보인다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unreadable;

/// 링크로 바뀌었거나 종류가 틀린 경로를 거부한다. 없는 경로는 아직 만들어지지 않은 기록이라 통과한다.
pub fn assert_plain_path(
    path: &Path,
    directory: bool,
) -> Result<(), Unreadable> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            let right_kind = if directory { metadata.is_dir() } else { metadata.is_file() };
            if metadata.file_type().is_symlink() || !right_kind { Err(Unreadable) } else { Ok(()) }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Unreadable),
    }
}

/// JSON 파일을 읽는다. 파일이 없으면 None이고, 읽을 수 없거나 깨졌으면 `Unreadable`이다(빈 값으로 숨기지 않는다).
pub fn read_json(path: &Path) -> Result<Option<Value>, Unreadable> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|_| Unreadable),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(Unreadable),
    }
}

/// 파일 끝 `N_TAIL_BYTES`만 글자로 읽는다. 중간에서 잘린 첫 줄은 JSON이 아니라 읽을 때 건너뛴다.
pub fn read_tail(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    let start = size.saturating_sub(N_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(N_TAIL_BYTES).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// 폴더 안 항목 이름을 내림차순으로. 링크는 종류(폴더·파일)에 들지 않아 따라가지 않는다. 폴더가 없으면 빈 목록이다.
fn newest_first(
    directory: &Path,
    want_directories: bool,
) -> std::io::Result<Vec<PathBuf>> {
    let listed = match std::fs::read_dir(directory) {
        Ok(entries) => Ok(entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_type().is_ok_and(|kind| if want_directories { kind.is_dir() } else { kind.is_file() })
            })
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<String>>()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    };
    listed.map(|mut names| {
        names.sort();
        names.reverse();
        names.into_iter().map(|name| directory.join(name)).collect()
    })
}

/// 가장 최근에 바뀐 Codex 세션 기록 파일(`<연>/<월>/<일>/*.jsonl`). 최근 이틀 폴더·50파일까지만 훑는다.
pub fn latest_codex_session(root: &Path) -> std::io::Result<Option<(PathBuf, i64)>> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut n_days = 0;
    'scan: for year in newest_first(root, true)? {
        for month in newest_first(&year, true)? {
            for day in newest_first(&month, true)? {
                let room = N_RECENT_FILES - files.len();
                files.extend(
                    newest_first(&day, false)?
                        .into_iter()
                        .filter(|file| file.extension().is_some_and(|extension| extension == "jsonl"))
                        .take(room),
                );
                n_days += 1;
                if n_days >= N_RECENT_DAYS || files.len() >= N_RECENT_FILES {
                    break 'scan;
                }
            }
        }
    }
    let mut latest: Option<(PathBuf, i64)> = None;
    for file in files {
        let modified = std::fs::metadata(&file)?
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0));
        if latest.as_ref().is_none_or(|(_, best)| modified > *best) {
            latest = Some((file, modified));
        }
    }
    Ok(latest)
}

/// 세션 기록 폴더들 가운데 가장 최근 기록의 한도. 기록이 없으면 빈 읽기 결과이고, 권한 오류는 `Unreadable`이다
/// (오래된 값과 헷갈리지 않게 조회 실패로 알린다).
pub fn recorded_codex(roots: &[PathBuf]) -> Result<Reading, Unreadable> {
    let mut best: Option<(PathBuf, i64)> = None;
    let mut failed = false;
    for root in roots {
        match latest_codex_session(root) {
            Ok(Some(found)) if best.as_ref().is_none_or(|(_, modified)| found.1 > *modified) => best = Some(found),
            Ok(_) => {}
            Err(_) => failed = true,
        }
    }
    if failed {
        Err(Unreadable)
    } else {
        match best {
            Some((file, modified)) => match read_tail(&file) {
                Ok(text) => {
                    Ok(Reading { windows: codex_windows(&text), updated_at: Some(modified), ..Reading::default() })
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Reading::default()),
                Err(_) => Err(Unreadable),
            },
            None => Ok(Reading::default()),
        }
    }
}

/// 숫자 JSON 값을 초 단위 정수로.
fn seconds_of(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_f64).map(|seconds| seconds as i64)
}

/// Claude 상태 줄이 남긴 기록(`rate_limits`·`updated_at`)에서 읽기 결과를 만든다.
fn claude_reading(
    record: Option<&Value>,
    state: ClaudeState,
) -> Reading {
    Reading {
        windows: record.map(claude_windows).unwrap_or_default(),
        updated_at: record.and_then(|record| seconds_of(record.get("updated_at"))),
        state,
        ..Reading::default()
    }
}

/// 사용량 표시 설정이 기록한 Claude 상태(`<설정 폴더>/usage/state.json`의 `claude`). 파일이 없으면 `Unset`이다.
fn claude_state() -> Result<ClaudeState, Unreadable> {
    let file = config_dir().join("usage").join("state.json");
    assert_plain_path(&file, false)?;
    let state = read_json(&file)?;
    Ok(state
        .as_ref()
        .and_then(|state| state.get("claude"))
        .and_then(Value::as_str)
        .map_or(ClaudeState::Unset, ClaudeState::parse))
}

/// 배포판 이름(None이면 기본 배포판)으로 그 이름과 홈 폴더 경로 규칙을 얻는 함수. WSL이 없거나 읽지 못하면 None을 돌려준다.
pub type WslLookup = Box<dyn Fn(Option<&str>) -> Option<(String, WslMapping)> + Send + Sync>;

/// 읽기 환경: WSL 배포판의 홈 폴더를 알려 주는 함수와 Codex 실시간 조회기.
pub struct ReadContext {
    pub wsl: WslLookup,
    pub codex: std::sync::Arc<CodexReader>,
}

/// 등록 계정 하나의 폴더들.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileDirs {
    /// 도구가 보는 설정 폴더(WSL이면 Linux 경로).
    pub config_dir: String,
    /// 이 앱이 읽는 경로들.
    pub usage_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub login_file: PathBuf,
}

/// 등록 계정의 폴더를 정하고 링크·종류를 확인한다. 읽는 것만 하고 폴더를 만들지 않는다.
///
/// 이 컴퓨터 계정은 `<설정 폴더>/agent-profiles/<id>/<도구>`, WSL 계정은 그 배포판 사용자 홈의 `~/.paddock/agent-profiles/<id>/<도구>`다.
/// 배포판을 못 읽거나 홈이 그 배포판 것이 아니면 `Unreadable`이다.
pub fn resolve_profile(
    profile: &AccountProfile,
    context: &ReadContext,
) -> Result<ProfileDirs, Unreadable> {
    let login_name = if profile.provider == "claude" { ".claude.json" } else { "auth.json" };
    let dirs;
    let managed: Vec<PathBuf>;
    if profile.runtime == Runtime::Wsl {
        let distribution = profile.wsl_distribution.as_deref().ok_or(Unreadable)?;
        let (_, mapping) = (context.wsl)(Some(distribution)).ok_or(Unreadable)?;
        let home = mapping.linux_home.trim_end_matches('/').to_string();
        let root = format!("{home}/.paddock/agent-profiles");
        let config = format!("{root}/{}/{}", profile.id, profile.provider);
        let local = |file: &str| mapping.to_local(file).map(PathBuf::from).ok_or(Unreadable);
        managed = vec![
            local(&format!("{home}/.paddock"))?,
            local(&root)?,
            local(&format!("{root}/{}", profile.id))?,
            local(&config)?,
        ];
        dirs = ProfileDirs {
            usage_dir: local(&format!("{config}/paddock-usage"))?,
            sessions_dir: local(&format!("{config}/sessions"))?,
            login_file: local(&format!("{config}/{login_name}"))?,
            config_dir: config,
        };
    } else {
        let root = config_dir().join("agent-profiles");
        let config = root.join(&profile.id).join(&profile.provider);
        managed = vec![config_dir(), root.clone(), root.join(&profile.id), config.clone()];
        dirs = ProfileDirs {
            usage_dir: config.join("paddock-usage"),
            sessions_dir: config.join("sessions"),
            login_file: config.join(login_name),
            config_dir: config.to_string_lossy().into_owned(),
        };
    }
    for directory in managed.iter().chain([&dirs.usage_dir, &dirs.sessions_dir]) {
        assert_plain_path(directory, true)?;
    }
    Ok(dirs)
}

/// 등록 Claude 계정의 기록: 그 계정 폴더의 `paddock-usage/claude.json`만 읽는다. 켜짐 여부는 이 Paddock 하나의 설정을 따른다.
pub fn read_claude_profile(dirs: &ProfileDirs) -> Result<Reading, Unreadable> {
    let usage_file = dirs.usage_dir.join("claude.json");
    assert_plain_path(&usage_file, false)?;
    let record = read_json(&usage_file)?;
    Ok(claude_reading(record.as_ref(), claude_state()?))
}

/// 이 컴퓨터의 기본 CLI 설정 폴더: 앱 프로세스의 계정 변수(`CLAUDE_CONFIG_DIR`·`CODEX_HOME`)와 홈 폴더로 정한다.
pub fn native_default_config_dir(provider: &str) -> PathBuf {
    let variable = if provider == "claude" { "CLAUDE_CONFIG_DIR" } else { "CODEX_HOME" };
    let configured = std::env::var(variable).ok();
    let home = std::env::home_dir().map(|home| home.to_string_lossy().into_owned());
    default_config_dir(provider, configured.as_deref(), home.as_deref()).map(PathBuf::from).unwrap_or_default()
}

/// 이 컴퓨터의 기본 CLI 로그인 정보 파일.
pub fn native_default_login_file(provider: &str) -> PathBuf {
    let name = if provider == "claude" { ".claude.json" } else { "auth.json" };
    native_default_config_dir(provider).join(name)
}

/// 기본 Claude 기록 한 곳.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaudeSource {
    pub usage_file: PathBuf,
    /// 이 기록을 남기는 환경의 로그인 정보 파일(`.claude.json`).
    pub login_file: PathBuf,
    pub runtime: Runtime,
    pub wsl_distribution: Option<String>,
}

/// 기본 Claude의 기록 위치: 이 컴퓨터의 `<설정 폴더>/usage/claude.json`, Windows 앱이면 기본 WSL 배포판의 같은 위치도 함께.
pub fn claude_sources(
    context: &ReadContext,
    host_is_windows: bool,
) -> Vec<ClaudeSource> {
    let mut sources = vec![ClaudeSource {
        usage_file: config_dir().join("usage").join("claude.json"),
        login_file: native_default_login_file("claude"),
        runtime: Runtime::Native,
        wsl_distribution: None,
    }];
    if host_is_windows && let Some((distribution, mapping)) = (context.wsl)(None) {
        let home = mapping.linux_home.trim_end_matches('/').to_string();
        let local = |file: String| mapping.to_local(&file).map(PathBuf::from);
        if let (Some(usage_file), Some(login_file)) = (
            local(format!("{home}/.paddock/config/usage/claude.json")),
            default_login_file("claude", None, Some(&home)).and_then(local),
        ) {
            sources.push(ClaudeSource {
                usage_file,
                login_file,
                runtime: Runtime::Wsl,
                wsl_distribution: Some(distribution),
            });
        }
    }
    sources
}

/// 기본 Claude: 기록이 있는 환경 가운데 가장 최근(`updated_at`) 기록을 쓴다.
pub fn read_claude_default(sources: &[ClaudeSource]) -> Result<Reading, Unreadable> {
    let mut newest: Option<Value> = None;
    for source in sources {
        assert_plain_path(&source.usage_file, false)?;
        if let Some(record) = read_json(&source.usage_file)? {
            let updated = |value: &Value| seconds_of(value.get("updated_at")).unwrap_or(0);
            if newest.as_ref().is_none_or(|best| updated(&record) > updated(best)) {
                newest = Some(record);
            }
        }
    }
    Ok(claude_reading(newest.as_ref(), claude_state()?))
}

/// 기본 Codex 환경: 이 컴퓨터의 `CODEX_HOME`(없으면 `~/.codex`), Windows 앱이면 기본 WSL 배포판의 `~/.codex`도 함께.
pub fn codex_default_envs(
    context: &ReadContext,
    host_is_windows: bool,
) -> Vec<CodexEnv> {
    let native = native_default_config_dir("codex");
    let mut envs = vec![CodexEnv {
        runtime: Runtime::Native,
        wsl_distribution: None,
        config_dir: native.to_string_lossy().into_owned(),
        sessions_dir: native.join("sessions").to_string_lossy().into_owned(),
    }];
    if host_is_windows && let Some((distribution, mapping)) = (context.wsl)(None) {
        let config = default_config_dir("codex", None, Some(&mapping.linux_home)).unwrap_or_default();
        if let Some(sessions) = mapping.to_local(&format!("{config}/sessions")) {
            envs.push(CodexEnv {
                runtime: Runtime::Wsl,
                wsl_distribution: Some(distribution),
                config_dir: config,
                sessions_dir: sessions,
            });
        }
    }
    envs
}

/// 환경 하나의 세션 기록 읽기.
fn recorded(env: &CodexEnv) -> Result<Reading, LookupFailed> {
    recorded_codex(&[PathBuf::from(&env.sessions_dir)]).map_err(|Unreadable| LookupFailed)
}

/// 범위 하나의 사용량을 읽는다(작업 스레드에서 부른다). 읽지 못하면 `Unavailable`이다.
///
/// 등록 계정은 자기 폴더만 읽고, 기본 CLI는 기본 환경만 읽는다 — 어느 쪽 값도 다른 쪽을 채우지 않는다.
/// 미등록 폴더는 읽을 곳이 없어 호출자가 읽지 않는다.
pub fn read_scope(
    scope: &Scope,
    context: &ReadContext,
    host_is_windows: bool,
    now_seconds: i64,
) -> Snapshot {
    let unavailable = Snapshot::Unavailable { message: None, removed: false };
    let result: Result<Reading, Unreadable> = match (&scope.profile, scope.provider.as_str()) {
        (Some(profile), "claude") => resolve_profile(profile, context).and_then(|dirs| read_claude_profile(&dirs)),
        (Some(profile), _) => resolve_profile(profile, context).and_then(|dirs| {
            let env = CodexEnv {
                runtime: profile.runtime,
                wsl_distribution: profile.wsl_distribution.clone(),
                config_dir: dirs.config_dir.clone(),
                sessions_dir: dirs.sessions_dir.to_string_lossy().into_owned(),
            };
            let live = context.codex.read(&env);
            let mut reading = live.clone().unwrap_or_default();
            let mut failed = live.is_err();
            if reading.windows.is_empty() {
                match recorded_codex(std::slice::from_ref(&dirs.sessions_dir)) {
                    // 실시간 값 대신 세션 기록을 쓰면 표시해, 화면이 지금 값처럼 보이지 않게 한다.
                    Ok(found) if !found.windows.is_empty() => {
                        reading = Reading { recorded: true, ..found };
                        failed = false;
                    }
                    Ok(_) => {}
                    Err(Unreadable) => failed = true,
                }
            }
            if reading.windows.is_empty() && failed { Err(Unreadable) } else { Ok(reading) }
        }),
        (None, "claude") => {
            let sources = claude_sources(context, host_is_windows);
            read_claude_default(&sources).map(|mut reading| {
                // 기본 Claude의 로그인이 등록 계정 하나와 같으면 그 계정으로 알리고, 같은 계정의 한도는 하나이므로
                // 그 계정 폴더에 더 최근에 받은 값이 있으면 그것을 쓴다.
                if let Some(linked) = link_of("claude", &claude_default_logins(&sources), context) {
                    let own = read_profiles()
                        .ok()
                        .and_then(|profiles| profiles.into_iter().find(|profile| profile.id == linked.id))
                        .and_then(|profile| resolve_profile(&profile, context).ok())
                        .and_then(|dirs| read_claude_profile(&dirs).ok());
                    if let Some(own) = own.filter(|own| own.updated_at.unwrap_or(-1) > reading.updated_at.unwrap_or(-1))
                    {
                        reading.windows = own.windows;
                        reading.updated_at = own.updated_at;
                    }
                    reading.account = Some(linked);
                }
                reading
            })
        }
        (None, _) => {
            let envs = codex_default_envs(context, host_is_windows);
            let mut reading =
                read_available(&envs, now_seconds, &mut |env| context.codex.read(env), &mut |env| recorded(env));
            reading.account = link_of("codex", &codex_default_logins(&envs), context);
            Ok(reading)
        }
    };
    match result {
        Ok(reading) => match &reading.error {
            Some(message) => Snapshot::Unavailable { message: Some(message.clone()), removed: false },
            None => Snapshot::Ready(reading),
        },
        Err(Unreadable) => unavailable,
    }
}

#[cfg(test)]
mod tests {
    use paddock_core::usage_model::Reset;

    use super::*;
    use crate::test_support::TempHome;

    fn write(
        path: &Path,
        text: &str,
    ) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn context() -> ReadContext {
        ReadContext { wsl: Box::new(|_| None), codex: std::sync::Arc::new(CodexReader::default()) }
    }

    fn profile(
        id: &str,
        provider: &str,
    ) -> AccountProfile {
        AccountProfile {
            id: id.to_string(),
            provider: provider.to_string(),
            label: id.to_string(),
            runtime: Runtime::Native,
            wsl_distribution: None,
        }
    }

    fn codex_record(used: u32) -> String {
        format!(
            "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"rate_limits\":{{\"primary\":{{\"used_percent\":{used},\"window_minutes\":300,\"resets_at\":1999999999}}}}}}}}\n"
        )
    }

    fn account_dir(profile: &AccountProfile) -> PathBuf {
        config_dir().join("agent-profiles").join(&profile.id).join(&profile.provider)
    }

    fn used(snapshot: &Snapshot) -> Option<i64> {
        match snapshot {
            Snapshot::Ready(reading) => reading.windows.first().map(|window| window.used),
            _ => None,
        }
    }

    #[test]
    fn test_au01_a_registered_account_with_no_records_remains_empty_and_lookup_creates_no_usage_files() {
        let _home = TempHome::new();
        let account = profile("a1", "claude");
        std::fs::create_dir_all(account_dir(&account)).unwrap();
        let snapshot = read_scope(&Scope::of_profile(&account), &context(), false, 1000);
        assert_eq!(snapshot, Snapshot::Ready(Reading::default()));
        assert!(!account_dir(&account).join("paddock-usage").exists(), "reading creates nothing");
    }

    #[test]
    fn test_au02_each_claude_account_reads_only_its_own_summary_and_never_fills_codex_with_a_default_account() {
        let _home = TempHome::new();
        let (first, second) = (profile("a1", "claude"), profile("a2", "claude"));
        write(
            &config_dir().join("usage").join("claude.json"),
            r#"{"rate_limits":{"five_hour":{"used_percentage":99}},"updated_at":1000}"#,
        );
        write(&config_dir().join("usage").join("state.json"), r#"{"claude":"on"}"#);
        write(
            &account_dir(&first).join("paddock-usage/claude.json"),
            r#"{"rate_limits":{"five_hour":{"used_percentage":12,"resets_at":9999}},"updated_at":100}"#,
        );
        write(
            &account_dir(&second).join("paddock-usage/claude.json"),
            r#"{"rate_limits":{"five_hour":{"used_percentage":43,"resets_at":9999}},"updated_at":200}"#,
        );
        let one = read_scope(&Scope::of_profile(&first), &context(), false, 1000);
        let two = read_scope(&Scope::of_profile(&second), &context(), false, 1000);
        assert_eq!((used(&one), used(&two)), (Some(12), Some(43)));
        assert!(
            matches!(&one, Snapshot::Ready(reading) if reading.state == ClaudeState::On && reading.updated_at == Some(100))
        );
        let default = read_scope(&Scope::provider("claude"), &context(), false, 1000);
        assert_eq!(used(&default), Some(99), "the default environment reads its own record");
    }

    #[test]
    fn test_au03_codex_reads_its_own_session_limits_and_never_returns_conversation_content() {
        let _home = TempHome::new();
        let (first, second) = (profile("c1", "codex"), profile("c2", "codex"));
        let session = account_dir(&first).join("sessions/2026/10/05/rollout-fixture.jsonl");
        write(&session, &codex_record(21));
        std::fs::OpenOptions::new()
            .append(true)
            .open(&session)
            .and_then(|mut file| {
                std::io::Write::write_all(
                    &mut file,
                    b"\n{\"type\":\"message\",\"text\":\"fixture conversation must stay local\"}\n",
                )
            })
            .unwrap();
        write(&account_dir(&second).join("sessions/2026/10/05/rollout-other.jsonl"), &codex_record(89));
        // 실시간 조회기는 Codex가 없는 시험 환경에서 실패하므로 세션 기록이 쓰인다.
        let snapshot = read_scope(&Scope::of_profile(&first), &context(), false, 1000);
        assert_eq!(used(&snapshot), Some(21));
        assert!(matches!(&snapshot, Snapshot::Ready(reading) if reading.recorded));
        assert!(!format!("{snapshot:?}").contains("fixture conversation"));
    }

    #[test]
    fn test_c_d74_a_registered_account_reads_only_its_own_folder() {
        let _home = TempHome::new();
        let account = profile("only", "claude");
        write(
            &config_dir().join("usage").join("claude.json"),
            r#"{"rate_limits":{"five_hour":{"used_percentage":88,"resets_at":9999999999}},"updated_at":5000}"#,
        );
        let snapshot = read_scope(&Scope::of_profile(&account), &context(), false, 1000);
        assert_eq!(
            used(&snapshot),
            None,
            "no record in the account folder means no value, not the default CLI's value"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_au10_redirected_usage_files_and_profile_directories_are_rejected() {
        let _home = TempHome::new();
        let account = profile("a1", "claude");
        let outside = config_dir().join("outside.json");
        write(&outside, "{}");
        std::fs::create_dir_all(account_dir(&account).join("paddock-usage")).unwrap();
        std::os::unix::fs::symlink(&outside, account_dir(&account).join("paddock-usage/claude.json")).unwrap();
        assert!(matches!(
            read_scope(&Scope::of_profile(&account), &context(), false, 1000),
            Snapshot::Unavailable { .. }
        ));
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "{}");
        let linked = profile("a2", "claude");
        std::fs::create_dir_all(config_dir().join("agent-profiles")).unwrap();
        std::os::unix::fs::symlink(config_dir(), config_dir().join("agent-profiles").join("a2")).unwrap();
        assert!(matches!(
            read_scope(&Scope::of_profile(&linked), &context(), false, 1000),
            Snapshot::Unavailable { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn test_au11_a_codex_scan_does_not_follow_a_session_file_link_outside_the_account() {
        let _home = TempHome::new();
        let account = profile("c1", "codex");
        let outside = config_dir().join("unrelated.jsonl");
        write(&outside, &codex_record(95));
        let day = account_dir(&account).join("sessions/2026/10/05");
        std::fs::create_dir_all(&day).unwrap();
        std::os::unix::fs::symlink(&outside, day.join("rollout-linked.jsonl")).unwrap();
        assert_eq!(recorded_codex(&[account_dir(&account).join("sessions")]), Ok(Reading::default()));
    }

    #[test]
    fn test_au13_malformed_or_unreadable_usage_records_are_unavailable_instead_of_silently_empty() {
        let _home = TempHome::new();
        let account = profile("a1", "claude");
        let record = account_dir(&account).join("paddock-usage/claude.json");
        write(&record, "{unfinished");
        assert!(matches!(
            read_scope(&Scope::of_profile(&account), &context(), false, 1000),
            Snapshot::Unavailable { .. }
        ));
        write(&record, r#"{"rate_limits":{}}"#);
        assert!(matches!(read_scope(&Scope::of_profile(&account), &context(), false, 1000), Snapshot::Ready(_)));
        write(&config_dir().join("usage").join("state.json"), "not json");
        assert!(
            matches!(read_scope(&Scope::of_profile(&account), &context(), false, 1000), Snapshot::Unavailable { .. }),
            "a broken setting file is not read as unset"
        );
    }

    #[test]
    fn test_au17_a_codex_usage_scan_stops_after_two_recent_day_folders() {
        let _home = TempHome::new();
        let account = profile("c1", "codex");
        let sessions = account_dir(&account).join("sessions");
        for (day, used) in [("01", 10), ("02", 20), ("03", 30)] {
            write(&sessions.join(format!("2026/10/{day}/rollout-fixture.jsonl")), &codex_record(used));
        }
        let (file, _) = latest_codex_session(&sessions).unwrap().unwrap();
        let shown = file.to_string_lossy().replace('\\', "/");
        assert!(shown.contains("2026/10/0") && !shown.contains("2026/10/01"), "{shown}");
        // 가장 오래된 날은 훑지 않는다: 새 두 날의 파일이 비어도 세 번째 날로 넘어가지 않는다.
        std::fs::remove_file(sessions.join("2026/10/03/rollout-fixture.jsonl")).unwrap();
        std::fs::remove_file(sessions.join("2026/10/02/rollout-fixture.jsonl")).unwrap();
        assert_eq!(latest_codex_session(&sessions).unwrap(), None);
    }

    #[test]
    fn test_au18_au19_codex_without_a_sessions_folder_is_unavailable_only_when_the_lookup_failed_and_nothing_is_recorded()
     {
        let _home = TempHome::new();
        let account = profile("c1", "codex");
        std::fs::create_dir_all(account_dir(&account)).unwrap();
        // 실시간 조회가 실패하고 기록도 없으면 0%가 아니라 읽지 못함이다.
        assert!(matches!(
            read_scope(&Scope::of_profile(&account), &context(), false, 1000),
            Snapshot::Unavailable { .. }
        ));
        write(&account_dir(&account).join("sessions/2026/10/05/rollout-fixture.jsonl"), &codex_record(21));
        assert_eq!(used(&read_scope(&Scope::of_profile(&account), &context(), false, 1000)), Some(21));
    }

    #[test]
    fn test_c_d74_the_default_claude_reads_the_newest_of_its_environments() {
        let _home = TempHome::new();
        let home = config_dir().join("usage");
        write(
            &home.join("claude.json"),
            r#"{"rate_limits":{"five_hour":{"used_percentage":10,"resets_at":9999999999}},"updated_at":100}"#,
        );
        let other = config_dir().join("other-env/claude.json");
        write(
            &other,
            r#"{"rate_limits":{"five_hour":{"used_percentage":70,"resets_at":9999999999}},"updated_at":300}"#,
        );
        let sources = [
            ClaudeSource {
                usage_file: home.join("claude.json"),
                login_file: PathBuf::new(),
                runtime: Runtime::Native,
                wsl_distribution: None,
            },
            ClaudeSource {
                usage_file: other,
                login_file: PathBuf::new(),
                runtime: Runtime::Wsl,
                wsl_distribution: Some("Ubuntu".to_string()),
            },
        ];
        let reading = read_claude_default(&sources).unwrap();
        assert_eq!((reading.windows[0].used, reading.windows[0].reset), (70, Reset::At(9999999999)));
        assert_eq!(reading.updated_at, Some(300));
    }
}
