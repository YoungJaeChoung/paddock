//! Windows 셸(Git Bash·PowerShell·명령 프롬프트)이 프롬프트를 띄울 때마다 현재 폴더를 알리게 하는 설정과, 그 알림을 읽는 순수 규칙.
//!
//! Windows는 다른 프로세스의 현재 폴더를 알려 주지 않아, 셸이 터미널 제어 문자(OSC 7: `ESC ] 7 ; 폴더 BEL`)로 직접 알리게 한다.

/// 폴더가 바뀔 때마다 알린다. Windows PowerShell 5.1에는 폴더 변경 훅이 없어 cd(Set-Location)·Push/Pop-Location을 감싸고,
/// 프롬프트에서도 한 번 더 알린다(다른 방법으로 폴더가 바뀐 경우). 알림은 파이프라인이 아니라 콘솔로 써서 명령 출력에 섞이지 않는다.
/// 원래 prompt 함수는 감싸서 그대로 부르므로 oh-my-posh 같은 사용자 프롬프트는 그대로 보인다.
pub(crate) const POWERSHELL_SCRIPT: &str = concat!(
    r#"function global:__PaddockCwd { [Console]::Write("$([char]27)]7;$($executionContext.SessionState.Path.CurrentLocation.ProviderPath)$([char]7)") }; "#,
    r#"foreach ($name in "Set-Location", "Push-Location", "Pop-Location") { Set-Item "function:global:$name" ([scriptblock]::Create("Microsoft.PowerShell.Management\$name @args; __PaddockCwd")) }; "#,
    r#"$global:__paddockPrompt = $function:prompt; "#,
    r#"function global:prompt { __PaddockCwd; & $global:__paddockPrompt }"#,
);

/// 명령을 실행하기 직전(DEBUG 트랩)과 프롬프트마다 폴더가 바뀌었으면 알린다. `cd 폴더 && claude`처럼 한 줄로 이어 실행해도 잡힌다.
/// 출력을 파일로 돌린 묶음 안에서는 알리지 않는다(표준 출력이 터미널일 때만 쓴다). /dev/tty를 따로 열면 셸 시작 직후의 입력 글자가 빠졌다.
/// 사용자가 DEBUG 트랩을 따로 쓰면 이 설정이 덮어쓴다.
const BASH_PROMPT_COMMAND: &str = concat!(
    r#"__paddock_cwd() { [ "$PWD" = "$__paddock_pwd" ] || ! [ -t 1 ] || { __paddock_pwd=$PWD; printf "\033]7;%s\007" "$PWD"; }; }; "#,
    "__paddock_cwd; ",
    "trap __paddock_cwd DEBUG",
);

/// 셸을 띄울 때 덧붙일 값.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CwdReportOptions {
    /// 덧붙일 환경 변수.
    pub env: Vec<(String, String)>,
    /// 원래 인자 뒤에 붙인 전체 인자.
    pub shell_args: Vec<String>,
}

/// PowerShell `-EncodedCommand`가 받는 형식(UTF-16LE를 base64로)으로 바꾼다.
pub fn encode_powershell(script: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    // 보조 평면 글자는 UTF-16 코드 단위 둘로 나뉜다. PowerShell은 두 단위를 모두 받는다.
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let value =
            group.iter().enumerate().fold(0u32, |value, (index, byte)| value | (u32::from(*byte) << (16 - 8 * index)));
        for index in 0..4 {
            if index <= group.len() {
                encoded.push(ALPHABET[((value >> (18 - 6 * index)) & 0x3f) as usize] as char);
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

fn executable_name(shell_path: &str) -> String {
    shell_path.rsplit(['/', '\\']).next().unwrap_or("").to_lowercase()
}

/// Git Bash처럼 bash 계열 셸 실행 파일(`bash.exe`·`sh.exe`)인지.
pub fn is_bash_shell(shell_path: &str) -> bool {
    matches!(executable_name(shell_path).as_str(), "bash.exe" | "sh.exe")
}

/// 셸 실행 파일에 맞춰 현재 폴더 알림을 켜는 옵션 추가분. 알 수 없는 셸이면 None이다.
///
/// - Git Bash(`bash.exe`·`sh.exe`): `PROMPT_COMMAND` 환경 변수로 DEBUG 트랩을 건다. 사용자 .bashrc가 `PROMPT_COMMAND`를 덮어쓰면 알림이 꺼진다.
/// - PowerShell(`powershell.exe`·`pwsh.exe`): 시작 명령으로 cd·prompt를 감싼다(`-NoExit -EncodedCommand`).
/// - 명령 프롬프트(`cmd.exe`): `PROMPT` 환경 변수에 알림을 앞에 붙인다. 훅이 없어 `cd … && claude`처럼 한 줄로 이으면 다음 프롬프트까지 늦게 알린다.
pub fn cwd_report_options(
    shell_path: &str,
    shell_args: &[String],
) -> Option<CwdReportOptions> {
    let name = executable_name(shell_path);
    if is_bash_shell(shell_path) {
        Some(CwdReportOptions {
            env: vec![("PROMPT_COMMAND".to_string(), BASH_PROMPT_COMMAND.to_string())],
            shell_args: shell_args.to_vec(),
        })
    } else if name == "powershell.exe" || name == "pwsh.exe" {
        let mut args = shell_args.to_vec();
        args.extend(["-NoExit".to_string(), "-EncodedCommand".to_string(), encode_powershell(POWERSHELL_SCRIPT)]);
        Some(CwdReportOptions { env: Vec::new(), shell_args: args })
    } else if name == "cmd.exe" {
        Some(CwdReportOptions {
            env: vec![("PROMPT".to_string(), "$E]7;$P$E\\$P$G".to_string())],
            shell_args: shell_args.to_vec(),
        })
    } else {
        None
    }
}

/// `%XX` 퍼센트 부호를 풀어 UTF-8 글자로 되돌린다. 깨진 부호는 그대로 둔다.
pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = |offset: usize| bytes.get(index + offset).and_then(|byte| (*byte as char).to_digit(16));
        if bytes[index] == b'%'
            && let (Some(high), Some(low)) = (hex(1), hex(2))
        {
            decoded.push((high * 16 + low) as u8);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// OSC 7 알림 내용을 슬래시 Windows 경로로 바꾼다. `file://` 주소·슬래시 경로·역슬래시 경로와 Git Bash 경로(`/c/…`)를 받고, 빈 값은 빈 글자다.
///
/// Examples
/// --------
/// | 알림 내용                       | 결과               |
/// |---------------------------------|--------------------|
/// | `C:/Users/me/app`               | `C:/Users/me/app`  |
/// | `C:\Users\me\app`               | `C:/Users/me/app`  |
/// | `/c/Users/me/app`               | `C:/Users/me/app`  |
/// | `file://host/C:/Users/me/a%20b` | `C:/Users/me/a b`  |
pub fn parse_cwd_report(data: &str) -> String {
    let mut path = data.trim().to_string();
    // 바이트 7에서 글자 경계가 깨지는 한글·이모지 경로에서도 멈추지 않도록 `get`으로 자른다.
    if path.get(..7).is_some_and(|scheme| scheme.eq_ignore_ascii_case("file://")) {
        let after_scheme = &path[7..];
        let without_host = after_scheme.find('/').map_or("", |slash| &after_scheme[slash..]);
        path = percent_decode(without_host);
        let bytes = path.as_bytes();
        if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
            path.remove(0);
        }
    }
    let path = path.replace('\\', "/");
    // Git Bash는 C:\Users를 /c/Users로 보인다. 한 글자 폴더 바로 아래만 드라이브로 본다(/home 같은 Linux 경로는 그대로).
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && (bytes.len() == 2 || bytes[2] == b'/')
    {
        let rest = if bytes.len() == 2 { "/" } else { &path[2..] };
        format!("{}:{rest}", (bytes[1] as char).to_ascii_uppercase())
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// base64 글자를 바이트로 되돌린다(시험에서 인코딩 결과를 확인하는 용도).
    fn decode_base64(text: &str) -> Vec<u8> {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut bytes = Vec::new();
        let mut buffer = 0u32;
        let mut n_bits = 0;
        for symbol in text.bytes().filter(|symbol| *symbol != b'=') {
            let value = ALPHABET.iter().position(|candidate| *candidate == symbol).expect("base64 symbol") as u32;
            buffer = (buffer << 6) | value;
            n_bits += 6;
            if n_bits >= 8 {
                n_bits -= 8;
                bytes.push((buffer >> n_bits) as u8);
                buffer &= (1 << n_bits) - 1;
            }
        }
        bytes
    }

    fn utf16le_text(base64: &str) -> String {
        let units: Vec<u16> =
            decode_base64(base64).chunks(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        String::from_utf16(&units).expect("utf16")
    }

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn test_c_cwd_3_powershell_keeps_complete_unicode_paths_in_the_startup_command() {
        let script = "$env:CODEX_HOME = 'C:\\Users\\작업😀\\accounts'";
        assert_eq!(utf16le_text(&encode_powershell(script)), script);
    }

    #[test]
    fn test_c_cwd_1_each_shell_gets_its_own_cwd_report_options_and_unknown_shells_none() {
        let bash =
            cwd_report_options("C:\\Program Files\\Git\\bin\\bash.exe", &args(&["--login", "-i"])).expect("bash");
        let prompt_command = &bash.env.iter().find(|(name, _)| name == "PROMPT_COMMAND").expect("PROMPT_COMMAND").1;
        assert!(prompt_command.contains("]7;"));
        assert!(prompt_command.contains("trap __paddock_cwd DEBUG"));
        assert_eq!(bash.shell_args, args(&["--login", "-i"]));

        let powershell =
            cwd_report_options("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe", &args(&["-NoLogo"]))
                .expect("powershell");
        assert_eq!(powershell.shell_args[..3], args(&["-NoLogo", "-NoExit", "-EncodedCommand"])[..]);
        let script = utf16le_text(&powershell.shell_args[3]);
        assert!(script.contains("function global:prompt"));
        assert!(script.contains("Microsoft.PowerShell.Management\\$name @args"));

        let cmd = cwd_report_options("C:\\Windows\\System32\\cmd.exe", &[]).expect("cmd");
        assert_eq!(cmd.env, vec![("PROMPT".to_string(), "$E]7;$P$E\\$P$G".to_string())]);
        assert!(cwd_report_options("C:\\Windows\\System32\\wsl.exe", &[]).is_none());
    }

    #[test]
    fn test_c_cwd_2_the_report_becomes_a_slash_windows_path() {
        assert_eq!(parse_cwd_report("C:/Users/me/app"), "C:/Users/me/app");
        assert_eq!(parse_cwd_report("C:\\Users\\me\\app"), "C:/Users/me/app");
        assert_eq!(parse_cwd_report("file://host/C:/Users/me/a%20b"), "C:/Users/me/a b");
        assert_eq!(parse_cwd_report("/c/Users/me/app"), "C:/Users/me/app");
        assert_eq!(parse_cwd_report("/d"), "D:/");
        assert_eq!(parse_cwd_report("/home/me"), "/home/me");
        assert_eq!(parse_cwd_report(""), "");
    }

    #[test]
    fn test_parse_cwd_report_does_not_split_multibyte_paths() {
        // 앞 7바이트 안에서 글자가 잘리는 경로(한글 3바이트, 이모지 4바이트)도 패닉 없이 그대로 돌려준다.
        assert_eq!(parse_cwd_report("/tmp/작"), "/tmp/작");
        assert_eq!(parse_cwd_report("/home/작업"), "/home/작업");
        assert_eq!(parse_cwd_report("/opt/🙂🙂"), "/opt/🙂🙂");
        assert_eq!(parse_cwd_report("작업"), "작업");
        assert_eq!(parse_cwd_report("file://host/home/작업/a%20b"), "/home/작업/a b");
        assert_eq!(parse_cwd_report("FILE:///tmp/🙂"), "/tmp/🙂");
    }
}
