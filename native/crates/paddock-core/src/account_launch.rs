//! 계정 터미널의 실행 명세: 등록 계정 하나를 Claude·Codex 공식 CLI가 도는 터미널로 띄우는 프로그램·인자·환경 변수.
//!
//! 로그인을 우회할 수 있는 환경 변수(`AUTH_ENV`)는 값을 읽지 않고 이름만으로 지우고, 계정 폴더는 도구별 변수
//! (`CLAUDE_CONFIG_DIR`·`CODEX_HOME`)로만 알린다. 경로·인자는 셸이 해석하는 문자열에 끼우지 않고 따옴표로 감싼 글자나
//! 프로세스 인자로만 넘기므로 공백·따옴표·`$(…)`가 든 경로도 글자 그대로 전달된다. 파일·프로세스 호출은 하지 않는다.
use serde::{Deserialize, Serialize};

use crate::agent_account::{AccountProfile, Runtime};
use crate::cwd_report::{POWERSHELL_SCRIPT, encode_powershell};
use crate::shell_catalog::wsl_folder;
use crate::work_model::same_folder;

/// 로그인을 우회할 수 있는 환경 변수 이름. 계정 터미널에서는 모두 지운다.
pub const AUTH_ENV: [&str; 13] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_PROFILE",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "OPENAI_API_KEY",
    "CODEX_API_KEY",
    "OPENAI_BASE_URL",
    "CODEX_AUTH_JSON",
];

/// WSL 안에서 Windows 기본 브라우저를 여는 도우미의 이름. 계정 폴더의 `paddock-bin`에 있을 때만 쓴다.
const BROWSER_HELPER: &str = "paddock-open-browser";

/// 계정 터미널의 PowerShell 실행 파일(Windows 기본 설치 경로).
const WINDOWS_POWERSHELL: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";

/// WSL 실행 파일(Windows 기본 설치 경로).
const WINDOWS_WSL: &str = r"C:\Windows\System32\wsl.exe";

/// 셸 대신 직접 띄울 프로그램의 실행 명세. 경로·인자는 셸 문자열을 거치지 않고 프로세스 인자로 넘어간다.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// 데몬 환경에 덧붙일 환경 변수.
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// 데몬 환경에서 지울 환경 변수 이름. 값은 읽지 않는다.
    #[serde(default)]
    pub env_remove: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// 계정 버튼으로 연 터미널의 계정. 일반 터미널이면 None이다.
    #[serde(default)]
    pub account: Option<AccountProfile>,
}

/// 계정 터미널을 열 수 없는 이유. 문장은 화면에 그대로 보인다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchError {
    InvalidProvider,
    InvalidConfigPath,
    InvalidSettingsPath,
    InvalidPermissions,
    InvalidSessionId,
    /// 대화 이어하기를 지원하지 않는 도구다(Claude만 지원).
    ResumeTool,
    /// WSL 계정은 Windows에서만 연다.
    WslNeedsWindows,
    /// 폴더가 계정과 다른 WSL 배포판에 있다.
    DifferentDistribution,
    /// Windows 계정으로 WSL 안의 폴더를 열 수 없다.
    WslFolderNeedsWslAccount,
}

impl std::fmt::Display for LaunchError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        let text = match self {
            Self::InvalidProvider => "Choose a Claude or Codex account.",
            Self::InvalidConfigPath => "The account configuration path is invalid.",
            Self::InvalidSettingsPath => "The account settings path is invalid.",
            Self::InvalidPermissions => "Choose ask, allowSkip, or skip for agent permissions.",
            Self::InvalidSessionId => "The conversation identifier is invalid.",
            Self::ResumeTool => "The conversation belongs to another CLI or environment.",
            Self::WslNeedsWindows => "Open this WSL account on its Windows device.",
            Self::DifferentDistribution => {
                "This folder belongs to a different WSL distribution. Open a terminal in the account\u{2019}s distribution first."
            }
            Self::WslFolderNeedsWslAccount => {
                "Choose a WSL account for this Linux folder, or open a Windows folder first."
            }
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for LaunchError {}

/// 에이전트 권한 확인 방식(설정 `paddock.agents.permissions`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Permissions {
    /// CLI 기본대로 묻는다.
    #[default]
    Ask,
    /// Claude가 대화 안에서 확인 생략으로 바꿀 수 있게 한다. Codex는 대화 안에서 바꾸므로 더하는 것이 없다.
    AllowSkip,
    /// 처음부터 확인을 생략한다.
    Skip,
}

impl Permissions {
    /// 설정 값(`ask`·`allowSkip`·`skip`)을 읽는다. 모르는 값이면 거부한다.
    pub fn parse(value: &str) -> Result<Self, LaunchError> {
        match value {
            "ask" => Ok(Self::Ask),
            "allowSkip" => Ok(Self::AllowSkip),
            "skip" => Ok(Self::Skip),
            _ => Err(LaunchError::InvalidPermissions),
        }
    }

    /// 도구 실행 인자에 더할 권한 플래그.
    fn flags(
        self,
        provider: &str,
    ) -> &'static [&'static str] {
        match (self, provider) {
            (Self::AllowSkip, "claude") => &["--allow-dangerously-skip-permissions"],
            (Self::Skip, "claude") => &["--dangerously-skip-permissions"],
            (Self::Skip, "codex") => &["--dangerously-bypass-approvals-and-sandbox"],
            _ => &[],
        }
    }
}

/// 계정 실행 환경 안의 경로.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    /// 절대 경로.
    Absolute(String),
    /// 실행 환경의 `$HOME` 아래 경로(앞 `/` 없이). WSL 안의 홈은 Windows 쪽에서 모르므로 셸이 풀게 한다.
    UnderHome(String),
}

impl Location {
    /// 이 경로 아래 이름 하나를 더한 경로.
    fn join(
        &self,
        name: &str,
    ) -> Self {
        match self {
            Self::Absolute(path) => Self::Absolute(format!("{}/{name}", path.trim_end_matches('/'))),
            Self::UnderHome(path) => Self::UnderHome(format!("{path}/{name}")),
        }
    }

    /// 경로 글자(홈 기준이면 홈 아래 부분).
    fn text(&self) -> &str {
        match self {
            Self::Absolute(path) | Self::UnderHome(path) => path,
        }
    }

    /// POSIX 셸의 한 단어. 홈 기준 경로는 `"$HOME"'/…'`처럼 홈만 셸이 풀고 나머지는 글자 그대로다.
    fn posix_word(&self) -> String {
        match self {
            Self::Absolute(path) => quote_posix(path),
            Self::UnderHome(path) => format!("\"$HOME\"{}", quote_posix(&format!("/{path}"))),
        }
    }
}

/// 계정 터미널을 띄우는 데 필요한 값.
#[derive(Clone, Debug)]
pub struct AccountLaunchInput<'a> {
    pub profile: &'a AccountProfile,
    /// 계정 폴더(`agent-profiles/<id>/<도구>`).
    pub config_dir: Location,
    /// 터미널을 시작할 폴더. WSL 계정이면 WSL 안의 경로다.
    pub cwd: &'a str,
    pub is_windows: bool,
    pub permissions: Permissions,
    /// Claude에 `--settings`로 덧붙일 Paddock 전용 설정 파일. 시작 때 파일이 있을 때만 붙는다.
    pub flag_settings_file: Option<Location>,
    /// 같은 계정 폴더에서 이어 열 Claude 대화 식별자.
    pub resume_session_id: Option<&'a str>,
}

/// 값을 POSIX 셸 한 단어로 감싼다(`'`는 `'"'"'`로 이어 붙인다).
pub fn quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// 값을 PowerShell 작은따옴표 글자로 감싼다.
pub fn quote_powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// 도구의 설정 폴더를 정하는 환경 변수 이름.
fn config_variable(provider: &str) -> &'static str {
    if provider == "claude" { "CLAUDE_CONFIG_DIR" } else { "CODEX_HOME" }
}

/// 조건이 거짓이면 `error`로 거부한다.
fn ensure(
    condition: bool,
    error: LaunchError,
) -> Result<(), LaunchError> {
    if condition { Ok(()) } else { Err(error) }
}

fn has_control(text: &str) -> bool {
    text.chars().any(crate::account_profile::is_control)
}

/// 대화 식별자 모양(8-4-4-4-12 자리 16진수, 대소문자 무관)인지.
fn is_session_id(value: &str) -> bool {
    value.split('-').map(str::len).eq([8, 4, 4, 4, 12]) && value.chars().all(|c| c == '-' || c.is_ascii_hexdigit())
}

/// Bash 시작 파일을 메모리에서 만들어, CLI가 끝난 뒤에도 계정 환경이 남은 셸로 들어가게 하는 인자.
///
/// 사용자의 별칭·PATH를 먼저 읽은 뒤 계정을 적용한다. 시작 파일은 파이프(`<(printf …)`)로 넘겨 CLI가 쥔 인증 파일 옆에
/// 실행 스크립트를 남기지 않는다. CLI는 첫 프롬프트에서 시작해 작업 제어가 준비되고, 에이전트가 셸의 앞쪽 프로세스로 보인다.
fn bash_arguments(
    provider: &str,
    config_dir: &Location,
    browser_directory: Option<&Location>,
    cli_arguments: &[String],
    flag_settings_file: Option<&Location>,
) -> Vec<String> {
    let name = provider_label(provider);
    let mut lines = vec![
        "if [ -f ~/.bashrc ]; then . ~/.bashrc; fi".to_string(),
        format!("unset {}", AUTH_ENV.join(" ")),
        format!("export {}={}", config_variable(provider), config_dir.posix_word()),
    ];
    if let Some(directory) = browser_directory {
        // WSL에서는 공식 CLI가 로그인 흐름을 모두 맡고, 열어야 할 주소만 Windows 기본 브라우저로 넘긴다. 도우미가 있을 때만 쓴다.
        lines.push(format!(
            "if [ -d {} ]; then export PATH={}:\"$PATH\"; export BROWSER={}; fi",
            directory.posix_word(),
            directory.posix_word(),
            quote_posix(BROWSER_HELPER)
        ));
    }
    lines.push(format!(
        "printf '\\n%s\\n' {}",
        quote_posix(&format!("{name} account environment. Sign in through the official CLI if prompted."))
    ));
    lines.extend([
        // 작업 제어는 첫 프롬프트에서 준비된다. rcfile 안에서 CLI를 바로 실행하면 Bash가 앞쪽 그룹으로 남아 에이전트가 가려진다.
        "__paddock_account_prompt=(\"${PROMPT_COMMAND[@]}\")".to_string(),
        "__paddock_start_account() {".to_string(),
        "PROMPT_COMMAND=(\"${__paddock_account_prompt[@]}\")".to_string(),
        "unset __paddock_account_prompt".to_string(),
        "unset -f __paddock_start_account".to_string(),
    ]);
    if let Some(file) = flag_settings_file {
        // 이 컴퓨터에 아직 쓰인 적 없는 설정 파일이면 CLI가 시작을 거부하지 않도록 건너뛴다.
        lines.push(format!(
            "__paddock_flag=(); if [ -f {} ]; then __paddock_flag=(--settings {}); fi",
            file.posix_word(),
            file.posix_word()
        ));
    }
    let flag = if flag_settings_file.is_some() { " \"${__paddock_flag[@]}\"" } else { "" };
    let arguments: String = cli_arguments.iter().map(|argument| format!(" {}", quote_posix(argument))).collect();
    lines.push(format!(
        "if command -v {provider} >/dev/null 2>&1; then command {provider}{flag}{arguments}; else printf '%s\\n' {}; fi",
        quote_posix(&format!("{name} CLI was not found. Install it in this environment, then run {provider}."))
    ));
    lines.push("}".to_string());
    lines.push("PROMPT_COMMAND=__paddock_start_account".to_string());
    let script = lines.join("\n");
    vec![
        "-l".to_string(),
        "-c".to_string(),
        format!("exec /bin/bash --rcfile <(printf '%s\\n' {}) -i", quote_posix(&script)),
    ]
}

/// Windows 네이티브 계정은 PowerShell을 띄워 계정 환경을 적용한다. 응용 프로그램만 찾아 별칭·함수에 가로채이지 않는다.
fn powershell_arguments(
    provider: &str,
    config_dir: &str,
    cli_arguments: &[String],
    flag_settings_file: Option<&str>,
) -> Vec<String> {
    let name = provider_label(provider);
    let mut parts = vec![POWERSHELL_SCRIPT.to_string()];
    parts.extend(AUTH_ENV.iter().map(|variable| format!("Remove-Item Env:{variable} -ErrorAction SilentlyContinue")));
    parts.push(format!("$env:{} = {}", config_variable(provider), quote_powershell(config_dir)));
    parts.push(format!(
        "Write-Host {}",
        quote_powershell(&format!("{name} account environment. Sign in through the official CLI if prompted."))
    ));
    parts.push(format!(
        "$paddockCli = Get-Command {provider} -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1"
    ));
    if let Some(file) = flag_settings_file {
        // Bash 시작 파일과 같은 규칙: 설정 파일이 있을 때만 --settings를 더한다.
        parts.push(format!(
            "$paddockFlag = @(); if (Test-Path -LiteralPath {0}) {{ $paddockFlag = @('--settings', {0}) }}",
            quote_powershell(file)
        ));
    }
    let flag = if flag_settings_file.is_some() { " @paddockFlag" } else { "" };
    let arguments: String = cli_arguments.iter().map(|argument| format!(" {}", quote_powershell(argument))).collect();
    parts.push(format!(
        "if ($paddockCli) {{ & $paddockCli.Source{flag}{arguments} }} else {{ Write-Host {} }}",
        quote_powershell(&format!("{name} CLI was not found. Install it in this environment, then run {provider}."))
    ));
    vec![
        "-NoLogo".to_string(),
        "-NoExit".to_string(),
        "-EncodedCommand".to_string(),
        encode_powershell(&parts.join("; ")),
    ]
}

/// 도구 이름의 화면 표기(`claude` → `Claude`).
fn provider_label(provider: &str) -> &'static str {
    crate::account_profile::provider_name(provider).unwrap_or("")
}

/// 등록 계정을 계정 터미널의 실행 명세로 바꾼다. 로그인 값은 어디에도 담기지 않는다.
///
/// 실행 환경별로 띄우는 프로그램이 다르다: 이 컴퓨터(Linux·macOS)는 `/bin/bash`, Windows 네이티브는 PowerShell,
/// WSL은 `wsl.exe -d <배포판> --cd <폴더> -e /bin/bash`. 지울 환경 변수(`env_remove`)에는 `AUTH_ENV`와 시작 파일을
/// 가로채는 `BASH_ENV`·`ENV`가 들어간다. WSL의 `env`는 비어 있다 — 계정 폴더는 WSL 안에서 시작 파일이 정한다.
///
/// Examples
/// --------
/// | 계정                  | is_windows | program                      | cwd         |
/// |-----------------------|------------|------------------------------|-------------|
/// | claude · native       | false      | `/bin/bash`                  | 입력 폴더   |
/// | codex · native        | true       | `powershell.exe` 전체 경로   | 입력 폴더   |
/// | codex · WSL Ubuntu    | true       | `wsl.exe` 전체 경로          | None(`--cd`) |
/// | codex · WSL Ubuntu    | false      | 오류 `WslNeedsWindows`       | -           |
pub fn account_terminal_spec(input: &AccountLaunchInput) -> Result<Launch, LaunchError> {
    let profile = input.profile;
    let provider = profile.provider.as_str();
    ensure(provider == "claude" || provider == "codex", LaunchError::InvalidProvider)?;
    let config_text = input.config_dir.text();
    ensure(!config_text.is_empty() && !has_control(config_text), LaunchError::InvalidConfigPath)?;
    if let Some(file) = &input.flag_settings_file {
        ensure(
            provider == "claude" && !file.text().is_empty() && !has_control(file.text()),
            LaunchError::InvalidSettingsPath,
        )?;
    }
    let mut cli_arguments: Vec<String> = input.permissions.flags(provider).iter().map(ToString::to_string).collect();
    if let Some(session_id) = input.resume_session_id {
        ensure(provider == "claude", LaunchError::ResumeTool)?;
        ensure(is_session_id(session_id), LaunchError::InvalidSessionId)?;
        cli_arguments.extend(["--resume".to_string(), session_id.to_string()]);
    }
    let mut env_remove: Vec<String> = AUTH_ENV.iter().map(ToString::to_string).collect();
    // 비대화형 셸 시작 파일이 첫 실행 명령을 가로채지 못하게 한다.
    env_remove.extend(["BASH_ENV".to_string(), "ENV".to_string()]);
    let (program, args, env, cwd) = match profile.runtime {
        Runtime::Wsl => {
            let distribution = profile
                .wsl_distribution
                .as_deref()
                .filter(|name| !name.is_empty() && input.is_windows)
                .ok_or(LaunchError::WslNeedsWindows)?;
            let browser = input.config_dir.join("paddock-bin");
            let mut args: Vec<String> =
                ["-d", distribution, "--cd", input.cwd, "-e", "/bin/bash"].iter().map(ToString::to_string).collect();
            args.extend(bash_arguments(
                provider,
                &input.config_dir,
                Some(&browser),
                &cli_arguments,
                input.flag_settings_file.as_ref(),
            ));
            (WINDOWS_WSL.to_string(), args, Vec::new(), None)
        }
        Runtime::Native => {
            let variable = (config_variable(provider).to_string(), config_text.to_string());
            if input.is_windows {
                let flag = input.flag_settings_file.as_ref().map(Location::text);
                let args = powershell_arguments(provider, config_text, &cli_arguments, flag);
                (WINDOWS_POWERSHELL.to_string(), args, vec![variable], Some(input.cwd.to_string()))
            } else {
                let args = bash_arguments(
                    provider,
                    &input.config_dir,
                    None,
                    &cli_arguments,
                    input.flag_settings_file.as_ref(),
                );
                ("/bin/bash".to_string(), args, vec![variable], Some(input.cwd.to_string()))
            }
        }
    };
    Ok(Launch {
        program,
        args,
        env,
        env_remove,
        cwd,
        title: Some(format!("{} \u{b7} {}", provider_label(provider), profile.label)),
        account: Some(profile.clone()),
    })
}

/// 계정 터미널을 열 폴더를 실행 환경에 맞는 경로로 바꾼다.
///
/// WSL 계정은 같은 배포판의 WSL 폴더면 그 안의 경로를, 홈이면 `~`를, Windows 폴더면 그 경로를 `--cd`로 받는다.
/// Windows 네이티브 계정은 WSL 안의 폴더를 열 수 없다.
///
/// Examples
/// --------
/// | 계정             | 폴더                                        | 결과                       |
/// |------------------|---------------------------------------------|----------------------------|
/// | WSL Ubuntu       | `\\wsl.localhost\Ubuntu\home\me\app`        | `/home/me/app`             |
/// | WSL Ubuntu       | `\\wsl.localhost\Debian\home\me`            | 오류 `DifferentDistribution` |
/// | WSL Ubuntu       | 홈 폴더                                     | `~`                        |
/// | native (Windows) | `\\wsl.localhost\Ubuntu\home\me`            | 오류 `WslFolderNeedsWslAccount` |
pub fn account_launch_cwd(
    profile: &AccountProfile,
    folder: &str,
    home: Option<&str>,
    is_windows: bool,
) -> Result<String, LaunchError> {
    let wsl = wsl_folder(folder);
    match (profile.runtime, wsl) {
        (Runtime::Wsl, Some((distribution, path))) => {
            let own = profile.wsl_distribution.as_deref().unwrap_or("");
            if distribution.to_lowercase() == own.to_lowercase() {
                Ok(path)
            } else {
                Err(LaunchError::DifferentDistribution)
            }
        }
        (Runtime::Wsl, None) if home.is_some_and(|home| same_folder(home, folder)) => Ok("~".to_string()),
        (Runtime::Native, Some(_)) if is_windows => Err(LaunchError::WslFolderNeedsWslAccount),
        _ => Ok(folder.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "00000000-0000-4000-8000-000000000001";
    const SESSION: &str = "00000000-0000-4000-8000-000000000013";

    fn profile(
        provider: &str,
        runtime: Runtime,
    ) -> AccountProfile {
        AccountProfile {
            id: ID.into(),
            provider: provider.into(),
            label: "Main".into(),
            runtime,
            wsl_distribution: (runtime == Runtime::Wsl).then(|| "Ubuntu Test".to_string()),
        }
    }

    fn input<'a>(
        profile: &'a AccountProfile,
        config_dir: &str,
        cwd: &'a str,
        is_windows: bool,
    ) -> AccountLaunchInput<'a> {
        AccountLaunchInput {
            profile,
            config_dir: Location::Absolute(config_dir.into()),
            cwd,
            is_windows,
            permissions: Permissions::Ask,
            flag_settings_file: None,
            resume_session_id: None,
        }
    }

    /// PowerShell `-EncodedCommand` 인자를 글자로 되돌린다.
    fn decoded(launch: &Launch) -> String {
        const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let encoded = launch.args.last().expect("encoded script");
        let mut bytes = Vec::new();
        let mut bits = 0u32;
        let mut n_bits = 0;
        for character in encoded.chars().filter(|character| *character != '=') {
            bits = (bits << 6) | ALPHABET.find(character).expect("base64") as u32;
            n_bits += 6;
            if n_bits >= 8 {
                n_bits -= 8;
                bytes.push((bits >> n_bits) as u8);
                bits &= (1 << n_bits) - 1;
            }
        }
        let units: Vec<u16> = bytes.chunks(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        String::from_utf16(&units).expect("utf16")
    }

    /// Bash 시작 파일 본문. 바깥 `printf`의 따옴표 이어 붙이기(`'"'"'`)를 되돌려 시작 파일에 실제로 들어가는 글자로 만든다.
    fn rcfile_script(launch: &Launch) -> String {
        launch.args.last().expect("bash arguments").replace("'\"'\"'", "'")
    }

    #[test]
    fn test_l01_launch_contains_account_metadata_and_clears_credential_selectors_without_copying_values() {
        for (provider, variable) in [("claude", "CLAUDE_CONFIG_DIR"), ("codex", "CODEX_HOME")] {
            let account = profile(provider, Runtime::Native);
            let launch = account_terminal_spec(&input(&account, "/tmp/account path", "/tmp/work", false)).unwrap();
            let name = if provider == "claude" { "Claude" } else { "Codex" };
            assert_eq!(launch.title.as_deref(), Some(format!("{name} \u{b7} Main").as_str()));
            assert_eq!(launch.account.as_ref(), Some(&account));
            assert_eq!(launch.cwd.as_deref(), Some("/tmp/work"));
            assert_eq!(launch.env, [(variable.to_string(), "/tmp/account path".to_string())]);
            for name in AUTH_ENV {
                assert!(launch.env_remove.iter().any(|removed| removed == name), "{name}");
                assert!(launch.env.iter().all(|(key, _)| key != name));
            }
            assert!(["BASH_ENV", "ENV"].iter().all(|name| launch.env_remove.iter().any(|removed| removed == name)));
            // 직렬화해도 같다(터미널 기록에 그대로 실린다).
            let round_trip: Launch = serde_json::from_str(&serde_json::to_string(&launch).unwrap()).unwrap();
            assert_eq!(round_trip, launch);
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_l02_posix_quoting_keeps_apostrophes_and_shell_syntax_as_literal_text() {
        let value = "/tmp/user's path; $(printf unexpected) `printf unexpected` \u{1f600}";
        let output = std::process::Command::new("sh")
            .args(["-c", &format!("printf '%s' {}", quote_posix(value))])
            .output()
            .expect("sh");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), value);
        assert_eq!(quote_powershell("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn test_l03_wsl_launch_keeps_the_distribution_and_passes_the_folder_as_a_separate_argument() {
        let account = profile("codex", Runtime::Wsl);
        let cwd = "/home/test/work with spaces";
        let mut wsl = input(&account, "", cwd, true);
        wsl.config_dir = Location::UnderHome(".paddock/agent-profiles/abc/codex".into());
        let launch = account_terminal_spec(&wsl).unwrap();
        assert_eq!(launch.args[..6], ["-d", "Ubuntu Test", "--cd", cwd, "-e", "/bin/bash"]);
        assert!(launch.program.ends_with("wsl.exe"));
        // 폴더는 WSL이 `--cd`로 받으므로 Windows 쪽 작업 폴더가 아니다.
        assert_eq!(launch.cwd, None);
        let script = rcfile_script(&launch);
        assert!(script.contains("export CODEX_HOME=\"$HOME\"'/.paddock/agent-profiles/abc/codex'"), "{script}");
        assert!(script.contains("paddock-open-browser"));
        wsl.is_windows = false;
        assert_eq!(account_terminal_spec(&wsl).unwrap_err(), LaunchError::WslNeedsWindows);
        let mut nameless = account.clone();
        nameless.wsl_distribution = None;
        assert_eq!(
            account_terminal_spec(&input(&nameless, "/x", cwd, true)).unwrap_err(),
            LaunchError::WslNeedsWindows
        );
    }

    #[test]
    fn test_l04_control_characters_in_the_config_path_and_unsupported_providers_fail_before_building_anything() {
        let claude = profile("claude", Runtime::Native);
        for config_dir in ["", "/tmp/a\nb", "/tmp/a\0b"] {
            assert_eq!(
                account_terminal_spec(&input(&claude, config_dir, "/tmp", false)).unwrap_err(),
                LaunchError::InvalidConfigPath
            );
        }
        let other = profile("other", Runtime::Native);
        assert_eq!(
            account_terminal_spec(&input(&other, "/tmp/profile", "/tmp", false)).unwrap_err(),
            LaunchError::InvalidProvider
        );
    }

    #[test]
    fn test_l05_windows_encoded_startup_keeps_supplementary_unicode_and_quotes_account_paths() {
        let account = profile("codex", Runtime::Native);
        let launch =
            account_terminal_spec(&input(&account, "C:\\Users\\O'Brien \u{1f600}\\account", "C:\\work", true)).unwrap();
        let script = decoded(&launch);
        assert!(script.contains("$env:CODEX_HOME = 'C:\\Users\\O''Brien \u{1f600}\\account'"), "{script}");
        assert!(launch.args.contains(&"-NoExit".to_string()));
        assert!(script.contains("Remove-Item Env:OPENAI_API_KEY"));
        assert!(script.contains("function global:prompt"));
        assert_eq!(launch.cwd.as_deref(), Some("C:\\work"));
    }

    #[test]
    fn test_l09_permission_modes_add_each_clis_own_flag_default_to_asking_and_reject_unknown_modes() {
        let claude = profile("claude", Runtime::Native);
        let codex = profile("codex", Runtime::Native);
        let script_of = |account: &AccountProfile, permissions: Permissions| {
            let mut request = input(account, "/tmp/account", "/tmp/work", false);
            request.permissions = permissions;
            rcfile_script(&account_terminal_spec(&request).unwrap())
        };
        assert!(!script_of(&claude, Permissions::default()).contains("dangerously"));
        assert!(script_of(&claude, Permissions::AllowSkip).contains("'--allow-dangerously-skip-permissions'"));
        assert!(!script_of(&codex, Permissions::AllowSkip).contains("dangerously"));
        assert!(script_of(&claude, Permissions::Skip).contains("'--dangerously-skip-permissions'"));
        assert!(script_of(&codex, Permissions::Skip).contains("'--dangerously-bypass-approvals-and-sandbox'"));
        assert_eq!(Permissions::parse("ask"), Ok(Permissions::Ask));
        assert_eq!(Permissions::parse("allowSkip"), Ok(Permissions::AllowSkip));
        assert_eq!(Permissions::parse("skip"), Ok(Permissions::Skip));
        assert_eq!(Permissions::parse("always"), Err(LaunchError::InvalidPermissions));
    }

    #[test]
    fn test_resume_l03_resuming_in_the_same_account_folder_passes_only_the_conversation_identifier() {
        let claude = profile("claude", Runtime::Native);
        let mut request = input(&claude, "/tmp/account", "/tmp/work", false);
        request.resume_session_id = Some(SESSION);
        let script = rcfile_script(&account_terminal_spec(&request).unwrap());
        assert!(script.contains(&format!("'--resume' '{SESSION}'")));
        assert!(!script.contains("transcript"));
        request.resume_session_id = Some("../x");
        assert_eq!(account_terminal_spec(&request).unwrap_err(), LaunchError::InvalidSessionId);
        let codex = profile("codex", Runtime::Native);
        let mut other = input(&codex, "/tmp/account", "/tmp/work", false);
        other.resume_session_id = Some(SESSION);
        assert_eq!(account_terminal_spec(&other).unwrap_err(), LaunchError::ResumeTool);
    }

    #[cfg(unix)]
    #[test]
    fn test_l10_claude_adds_settings_only_when_the_flag_file_exists_in_the_bash_wsl_and_powershell_launchers() {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let directory = std::env::temp_dir().join(format!("paddock flag launch {}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let flag_file = directory.join("flag settings's.json");
        let claude = profile("claude", Runtime::Native);
        let config_dir = directory.to_string_lossy().into_owned();
        let mut request = input(&claude, &config_dir, &config_dir, false);
        request.flag_settings_file = Some(Location::Absolute(flag_file.to_string_lossy().into_owned()));
        // 상속한 `command` 함수가 CLI 자리에 서서 시작 파일이 CLI에 준 인자를 보고한다.
        let run = |launch: &Launch| {
            let function = "() { case \"$1\" in claude) shift; printf \"ARGS|%s\\n\" \"$*\" ;; -v) if [ \"$2\" = claude ]; then printf \"%s\\n\" \"$2\"; else builtin command \"$@\"; fi ;; *) builtin command \"$@\" ;; esac; }";
            let mut command = Command::new(&launch.program);
            command
                .args(&launch.args)
                .current_dir(&directory)
                .env("HOME", &directory)
                .env("HISTFILE", directory.join("history"))
                .env("BASH_FUNC_command%%", function)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            for (name, value) in &launch.env {
                command.env(name, value);
            }
            let mut child = command.spawn().expect("bash");
            child.stdin.take().unwrap().write_all(b"exit\n").unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            String::from_utf8_lossy(&output.stdout).lines().find(|line| line.starts_with("ARGS|")).map(str::to_string)
        };
        let launch = account_terminal_spec(&request).unwrap();
        assert_eq!(run(&launch).as_deref(), Some("ARGS|"));
        std::fs::write(&flag_file, "{}").unwrap();
        assert_eq!(run(&launch).as_deref(), Some(format!("ARGS|--settings {}", flag_file.display()).as_str()));
        request.resume_session_id = Some("00000000-0000-4000-8000-000000000014");
        let resumed = account_terminal_spec(&request).unwrap();
        assert_eq!(
            run(&resumed).as_deref(),
            Some(
                format!("ARGS|--settings {} --resume 00000000-0000-4000-8000-000000000014", flag_file.display())
                    .as_str()
            )
        );
        std::fs::remove_dir_all(&directory).unwrap();

        // WSL은 같은 Bash 시작 파일을 홈 기준 경로로 쓰고, PowerShell은 경로를 확인한 뒤에만 플래그를 더한다.
        let wsl_account = profile("claude", Runtime::Wsl);
        let mut wsl = input(&wsl_account, "", "/home/test/work", true);
        wsl.config_dir = Location::UnderHome(".paddock/agent-profiles/abc/claude".into());
        wsl.flag_settings_file = Some(Location::UnderHome(".paddock/config/claude/flag-settings.json".into()));
        let script = rcfile_script(&account_terminal_spec(&wsl).unwrap());
        assert!(
            script.contains("--settings") && script.contains("\"$HOME\"'/.paddock/config/claude/flag-settings.json'")
        );
        let mut windows = input(&claude, "C:\\accounts\\claude", "C:\\work", true);
        windows.flag_settings_file =
            Some(Location::Absolute("C:\\Users\\O'Brien\\.paddock\\config\\claude\\flag-settings.json".into()));
        let script = decoded(&account_terminal_spec(&windows).unwrap());
        assert!(
            script
                .contains("Test-Path -LiteralPath 'C:\\Users\\O''Brien\\.paddock\\config\\claude\\flag-settings.json'")
        );
        assert!(script.contains("& $paddockCli.Source @paddockFlag"));
        // 설정 파일이 없으면 --settings는 어디에도 없고, Codex는 설정 파일을 받지 않는다.
        let plain = account_terminal_spec(&input(&claude, "/tmp/x", "/tmp", false)).unwrap();
        assert!(!plain.args.last().unwrap().contains("--settings"));
        let codex = profile("codex", Runtime::Native);
        let mut with_flag = input(&codex, "/tmp/x", "/tmp", false);
        with_flag.flag_settings_file = Some(Location::Absolute("/tmp/f.json".into()));
        assert_eq!(account_terminal_spec(&with_flag).unwrap_err(), LaunchError::InvalidSettingsPath);
        let mut control = input(&claude, "/tmp/x", "/tmp", false);
        control.flag_settings_file = Some(Location::Absolute("/tmp/a\nb".into()));
        assert_eq!(account_terminal_spec(&control).unwrap_err(), LaunchError::InvalidSettingsPath);
    }

    #[test]
    fn test_c_d45_the_home_folder_is_recognised_whatever_the_separator_or_case() {
        let wsl = profile("claude", Runtime::Wsl);
        let cwd = |folder: &str| account_launch_cwd(&wsl, folder, Some("C:\\Users\\Me"), true);
        assert_eq!(cwd("C:/Users/me"), Ok("~".to_string()));
        assert_eq!(cwd("c:\\users\\me\\"), Ok("~".to_string()));
        assert_eq!(cwd("C:/Users/other"), Ok("C:/Users/other".to_string()));
    }

    #[test]
    fn test_launch_cwd_follows_the_account_environment() {
        let wsl = profile("claude", Runtime::Wsl);
        let native = profile("claude", Runtime::Native);
        let folder = r"\\wsl.localhost\Ubuntu Test\home\me\app";
        assert_eq!(account_launch_cwd(&wsl, folder, None, true).as_deref(), Ok("/home/me/app"));
        assert_eq!(
            account_launch_cwd(&wsl, r"\\wsl.localhost\Debian\home\me", None, true),
            Err(LaunchError::DifferentDistribution)
        );
        assert_eq!(account_launch_cwd(&wsl, r"C:\Users\me", Some(r"C:\Users\me"), true).as_deref(), Ok("~"));
        assert_eq!(account_launch_cwd(&wsl, r"C:\work", Some(r"C:\Users\me"), true).as_deref(), Ok(r"C:\work"));
        assert_eq!(account_launch_cwd(&native, folder, None, true), Err(LaunchError::WslFolderNeedsWslAccount));
        assert_eq!(account_launch_cwd(&native, "/home/me", None, false).as_deref(), Ok("/home/me"));
    }
}
