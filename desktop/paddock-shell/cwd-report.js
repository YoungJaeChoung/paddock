// Windows 셸(Git Bash·PowerShell·명령 프롬프트)이 프롬프트를 띄울 때마다 현재 폴더를 알리게 하는 설정과, 그 알림을 읽는 순수 함수.
// Windows는 다른 프로세스의 현재 폴더를 알려 주지 않아, 셸이 터미널 제어 문자(OSC 7: ESC ] 7 ; 폴더 BEL)로 직접 알리게 한다.

/** PowerShell -EncodedCommand가 받는 형식(UTF-16LE를 base64로). 브라우저·Node 양쪽에서 돈다. */
function encodePowerShell(
    script,
) {
    let binary = '';
    // PowerShell expects UTF-16 code units, including both halves of supplementary characters.
    for (let offset = 0; offset < script.length; offset += 1) {
        const code = script.charCodeAt(offset);
        binary += String.fromCharCode(code & 0xff, code >> 8);
    }
    return btoa(binary);
}

// 폴더가 바뀔 때마다 알린다. Windows PowerShell 5.1에는 폴더 변경 훅이 없어 cd(Set-Location)·Push/Pop-Location을 감싸고,
// 프롬프트에서도 한 번 더 알린다(다른 방법으로 폴더가 바뀐 경우). 알림은 파이프라인이 아니라 콘솔로 써서 명령 출력에 섞이지 않는다.
// 원래 prompt 함수는 감싸서 그대로 부르므로 oh-my-posh 같은 사용자 프롬프트는 그대로 보인다.
const POWERSHELL_SCRIPT = [
    'function global:__PaddockCwd { [Console]::Write("$([char]27)]7;$($executionContext.SessionState.Path.CurrentLocation.ProviderPath)$([char]7)") }',
    'foreach ($name in "Set-Location", "Push-Location", "Pop-Location") { Set-Item "function:global:$name" ([scriptblock]::Create("Microsoft.PowerShell.Management\\$name @args; __PaddockCwd")) }',
    '$global:__paddockPrompt = $function:prompt',
    'function global:prompt { __PaddockCwd; & $global:__paddockPrompt }',
].join('; ');

// 명령을 실행하기 직전(DEBUG 트랩)과 프롬프트마다 폴더가 바뀌었으면 알린다. `cd 폴더 && claude`처럼 한 줄로 이어 실행해도 잡힌다.
// 출력을 파일로 돌린 묶음 안에서는 알리지 않는다(표준 출력이 터미널일 때만 쓴다). /dev/tty를 따로 열면 셸 시작 직후의 입력 글자가 빠졌다.
// 사용자가 DEBUG 트랩을 따로 쓰면 이 설정이 덮어쓴다.
const BASH_PROMPT_COMMAND = [
    '__paddock_cwd() { [ "$PWD" = "$__paddock_pwd" ] || ! [ -t 1 ] || { __paddock_pwd=$PWD; printf "\\033]7;%s\\007" "$PWD"; }; }',
    '__paddock_cwd',
    'trap __paddock_cwd DEBUG',
].join('; ');

/**
 * 셸 실행 파일에 맞춰 현재 폴더 알림을 켜는 터미널 옵션 추가분 `{ env, shellArgs }`. 알 수 없는 셸이면 null이다.
 * - Git Bash(bash.exe·sh.exe): PROMPT_COMMAND 환경 변수로 DEBUG 트랩을 건다. 사용자 .bashrc가 PROMPT_COMMAND를 덮어쓰면 알림이 꺼진다.
 * - PowerShell(powershell.exe·pwsh.exe): 시작 명령으로 cd·prompt를 감싼다(-NoExit -EncodedCommand).
 * - 명령 프롬프트(cmd.exe): PROMPT 환경 변수에 알림을 앞에 붙인다. 훅이 없어 `cd … && claude`처럼 한 줄로 이으면 다음 프롬프트까지 늦게 알린다.
 * `shellArgs`는 원래 인자 뒤에 붙인 전체 인자 배열이다.
 */
/** Git Bash처럼 bash 계열 셸 실행 파일(bash.exe·sh.exe)인지. */
function isBashShell(
    shellPath,
) {
    return ['bash.exe', 'sh.exe'].includes(String(shellPath || '').split(/[\\/]/).pop().toLowerCase());
}

function cwdReportOptions(
    shellPath,
    shellArgs = [],
) {
    const name = String(shellPath || '').split(/[\\/]/).pop().toLowerCase();
    let options = null;
    if (isBashShell(shellPath)) {
        options = { env: { PROMPT_COMMAND: BASH_PROMPT_COMMAND }, shellArgs };
    } else if (name === 'powershell.exe' || name === 'pwsh.exe') {
        options = { env: {}, shellArgs: [...shellArgs, '-NoExit', '-EncodedCommand', encodePowerShell(POWERSHELL_SCRIPT)] };
    } else if (name === 'cmd.exe') {
        options = { env: { PROMPT: '$E]7;$P$E\\$P$G' }, shellArgs };
    }
    return options;
}

/**
 * OSC 7 알림 내용을 Windows 경로로 바꾼다. file:// 주소·슬래시 경로·역슬래시 경로와 Git Bash 경로(/c/…)를 받고, 빈 값은 빈 문자열이다.
 *
 * Examples
 * --------
 * | 알림 내용                         | 결과                  |
 * | --------------------------------- | --------------------- |
 * | `C:/Users/me/app`                 | `C:/Users/me/app`     |
 * | `C:\Users\me\app`                 | `C:/Users/me/app`     |
 * | `/c/Users/me/app`                 | `C:/Users/me/app`     |
 * | `file://host/C:/Users/me/a%20b`   | `C:/Users/me/a b`     |
 */
function parseCwdReport(
    data,
) {
    let path = String(data || '').trim();
    if (/^file:\/\//i.test(path)) {
        path = decodeURIComponent(path.replace(/^file:\/\/[^/]*/i, '')).replace(/^\/([A-Za-z]:)/, '$1');
    }
    // Git Bash는 C:\Users를 /c/Users로 보인다. 한 글자 폴더 바로 아래만 드라이브로 본다(/home 같은 Linux 경로는 그대로).
    return path.replace(/\\/g, '/').replace(/^\/([A-Za-z])(\/|$)/, (match, drive, rest) => `${drive.toUpperCase()}:${rest || '/'}`);
}

module.exports = { encodePowerShell, isBashShell, cwdReportOptions, parseCwdReport };
