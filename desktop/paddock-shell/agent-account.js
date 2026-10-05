// 터미널에서 실행 중인 claude·codex가 어느 계정 폴더로 돌고 있는지 정하는 순수 함수.
// 계정은 "어느 버튼으로 연 터미널인가"가 아니라 에이전트 프로세스 자신의 환경 변수로 정한다.
// 사용자가 터미널에서 `export CODEX_HOME=…; codex`처럼 직접 지정해도, 계정 터미널에서 다른 도구를 실행해도 같은 규칙이 적용된다.
// 백엔드(프로세스 환경 판정)와 프런트엔드(표시할 범위 고르기)가 함께 쓰므로 Node 전용 모듈을 불러오지 않는다.

class AgentAccount {
    /** 도구별로 설정 폴더를 정하는 환경 변수. 이 값이 없으면 도구는 `$HOME/<기본 폴더>`를 쓴다. */
    static VARIABLES = Object.freeze({ claude: 'CLAUDE_CONFIG_DIR', codex: 'CODEX_HOME' });
    static DEFAULT_FOLDERS = Object.freeze({ claude: '.claude', codex: '.codex' });
    /** 프로세스 환경에서 꺼내는 이름. 나머지 변수(로그인 키 등)는 값을 읽지 않고 버린다. */
    static READ = Object.freeze(['CLAUDE_CONFIG_DIR', 'CODEX_HOME', 'HOME']);
    static KINDS = Object.freeze({ DEFAULT: 'default', PROFILE: 'profile', CUSTOM: 'custom' });
    static CUSTOM_NAME = 'Unregistered folder';
    static CURRENT_NAME = 'Current CLI';
}

/**
 * 환경 목록(`이름=값`을 구분자로 이은 문자열, 기본은 /proc/<pid>/environ의 NUL)에서 계정 판정에 쓰는 세 값만 꺼낸다.
 * 빈 값은 없는 것으로 본다. 다른 변수는 결과에 들어가지 않는다.
 *
 * Examples
 * --------
 * | 입력                                               | 결과                                   |
 * | -------------------------------------------------- | -------------------------------------- |
 * | `'HOME=/home/me\0CODEX_HOME=/x\0OPENAI_API_KEY=k'` | `{ HOME: '/home/me', CODEX_HOME: '/x' }` |
 * | `'CLAUDE_CONFIG_DIR=\0'`                           | `{}`                                   |
 */
function parseEnviron(
    text,
    separator = '\0',
) {
    const env = {};
    for (const entry of String(text || '').split(separator)) {
        const index = entry.indexOf('=');
        const name = index > 0 ? entry.slice(0, index) : '';
        const value = entry.slice(index + 1);
        if (AgentAccount.READ.includes(name) && value) env[name] = value;
    }
    return env;
}

/**
 * Linux 경로를 비교용으로 다듬는다: 겹친 `/`, `.`, `..`, 끝 `/`를 정리한다. 절대 경로가 아니면 빈 문자열이다.
 * 심볼릭 링크 해석은 하지 않는다 — 실제 경로가 필요하면 호출하는 쪽이 realPath로 넘긴다.
 */
function normalizePath(
    file,
) {
    const value = String(file || '');
    let result = '';
    if (value.startsWith('/')) {
        const parts = [];
        for (const part of value.split('/')) {
            if (part === '..') parts.pop();
            else if (part && part !== '.') parts.push(part);
        }
        result = `/${parts.join('/')}`;
    }
    return result;
}

/**
 * 실행 중인 에이전트가 쓰는 계정을 정한다. 계정 표시·사용량·대화 식별이 모두 이 결과를 따른다.
 *
 * 판정 규칙(앞에서 맞는 것이 결과):
 * 1. 계정 변수가 없거나 그 실행 환경의 기본 폴더(`$HOME/.codex`·`$HOME/.claude`, 또는 defaultDirectories)와 같으면 기본 CLI(`default`).
 * 2. 등록 계정 폴더와 실제 경로가 같고 도구·실행 환경(native/wsl)·WSL 배포판이 맞으면 그 계정(`profile`).
 *    wslDistribution을 모르면(빈 값) 배포판은 비교하지 않는다.
 * 3. 그 밖의 폴더는 미등록 폴더(`custom`). 기본 CLI의 사용량이나 계정을 빌려 쓰지 않는다.
 *
 * Parameters
 * ----------
 * provider : 'claude' | 'codex'. 다른 값이면 null을 돌려준다.
 * runtime : 'native' | 'wsl'. 에이전트가 도는 환경.
 * wslDistribution : WSL 배포판 이름. 모르면 빈 값.
 * env : parseEnviron 결과(`CODEX_HOME`·`CLAUDE_CONFIG_DIR`·`HOME`).
 * profiles : 등록 계정 목록(`{ id, provider, runtime, wslDistribution? }`).
 * profileDirectory : 계정 → 그 실행 환경에서 본 계정 폴더 경로.
 * defaultDirectories : `$HOME` 기본 폴더 말고도 기본 CLI로 볼 폴더(앱이 물려준 CODEX_HOME 등).
 * realPath : 경로 → 실제 경로. 읽을 수 없으면 입력을 그대로 돌려줘야 한다.
 *
 * Returns
 * -------
 * `{ kind: 'default' }` | `{ kind: 'profile', profile }` | `{ kind: 'custom' }` | null
 *
 * Examples
 * --------
 * | CODEX_HOME                                   | 결과                   |
 * | -------------------------------------------- | ---------------------- |
 * | (없음)                                       | `{ kind: 'default' }`  |
 * | `/home/me/.codex/`                           | `{ kind: 'default' }`  |
 * | `<config>/agent-profiles/<codex 계정 id>/codex` | `{ kind: 'profile', profile }` |
 * | `<config>/agent-profiles/<claude 계정 id>/claude` | `{ kind: 'custom' }` |
 * | `/tmp/other`                                 | `{ kind: 'custom' }`   |
 */
function attributeAccount(
    {
        provider,
        runtime,
        wslDistribution = '',
        env,
        profiles = [],
        profileDirectory,
        defaultDirectories = [],
        realPath = file => file,
    },
) {
    let result = null;
    if (Object.hasOwn(AgentAccount.VARIABLES, provider)) {
        const canonical = file => {
            const normalized = normalizePath(file);
            return normalized && normalizePath(realPath(normalized));
        };
        const configured = canonical(env?.[AgentAccount.VARIABLES[provider]]);
        const home = normalizePath(env?.HOME);
        const defaults = [...(home ? [`${home === '/' ? '' : home}/${AgentAccount.DEFAULT_FOLDERS[provider]}`] : []), ...defaultDirectories].map(canonical).filter(Boolean);
        const profile = configured && profiles.find(item => item.provider === provider && item.runtime === runtime
            && (runtime !== 'wsl' || !wslDistribution || String(item.wslDistribution || '').toLowerCase() === wslDistribution.toLowerCase())
            && canonical(profileDirectory(item)) === configured);
        if (!configured && !env?.[AgentAccount.VARIABLES[provider]]) result = { kind: AgentAccount.KINDS.DEFAULT };
        else if (configured && defaults.includes(configured)) result = { kind: AgentAccount.KINDS.DEFAULT };
        else if (profile) result = { kind: AgentAccount.KINDS.PROFILE, profile };
        else result = { kind: AgentAccount.KINDS.CUSTOM };
    }
    return result;
}

/**
 * 화면(상태 줄·사용량·사이드바)이 이 터미널에 대해 보일 계정 범위를 고른다.
 * 실행 중인 에이전트가 있으면 그 에이전트의 판정 결과가 이기고, 없으면 터미널을 연 계정이다.
 *
 * Parameters
 * ----------
 * program : 터미널의 앞쪽 프로그램 이름(모르면 빈 값).
 * launchProfile : 계정 버튼으로 연 터미널의 계정(`options.paddockAccount`). 일반 터미널이면 없음.
 * detected : 백엔드가 에이전트 프로세스 환경으로 판정한 결과(attributeAccount 모양). 환경을 읽지 못했으면 없음.
 *   기본 CLI 판정에 profile이 붙어 있으면 그 로그인이 등록 계정 하나와 같다고 백엔드가 확인한 것이다.
 *
 * Returns
 * -------
 * 사용량 패널이 고를 범위 — 등록 계정 `{ id, provider, label, … }`, 기본 CLI `{ provider }`, 미등록 폴더 `{ provider, custom: true }` —
 * 또는 null(에이전트도 계정도 없음).
 * 로그인이 확인된 기본 CLI는 `{ provider, linked: { id, label } }`이다. 이름·사용량은 그 계정으로 보이지만 범위는 기본 CLI 그대로라,
 * 대화 기록·계정 전환은 기본 폴더를 쓴다(id가 없으므로 등록 계정 범위로 취급되지 않는다).
 *
 * Examples
 * --------
 * | program | launchProfile        | detected                  | 결과                         |
 * | ------- | -------------------- | ------------------------- | ---------------------------- |
 * | bash    | Work Claude          | -                         | Work Claude                  |
 * | claude  | Work Claude          | profile(Work Claude)      | Work Claude                  |
 * | codex   | Work Claude          | default                   | `{ provider: 'codex' }`       |
 * | codex   | -                    | default + Work Codex 로그인 | `{ provider: 'codex', linked: Work Codex }` |
 * | codex   | Work Claude          | (읽지 못함)               | `{ provider: 'codex' }`       |
 * | claude  | Work Claude          | (읽지 못함)               | Work Claude                  |
 * | codex   | -                    | profile(Work Codex)       | Work Codex                   |
 * | codex   | -                    | custom                    | `{ provider: 'codex', custom: true }` |
 * | bash    | -                    | -                         | null                         |
 */
function runningScope(
    {
        program,
        launchProfile,
        detected,
    },
) {
    let scope = launchProfile || null;
    if (Object.hasOwn(AgentAccount.VARIABLES, program)) {
        if (detected?.kind === AgentAccount.KINDS.PROFILE && detected.profile?.provider === program) {
            scope = launchProfile?.id === detected.profile.id ? launchProfile : detected.profile;
        } else if (detected?.kind === AgentAccount.KINDS.CUSTOM) {
            scope = { provider: program, custom: true };
        } else if (detected?.kind === AgentAccount.KINDS.DEFAULT && detected.profile?.provider === program) {
            scope = { provider: program, linked: { id: detected.profile.id, label: detected.profile.label } };
        } else if (detected?.kind === AgentAccount.KINDS.DEFAULT || launchProfile?.provider !== program) {
            scope = { provider: program };
        }
    }
    return scope;
}

/**
 * 범위의 계정 이름: 등록 계정은 그 이름, 미등록 폴더는 'Unregistered folder', 기본 범위는 'Current CLI'.
 * 기본 범위라도 로그인이 등록 계정 하나와 같다고 확인됐으면(linked) 그 계정 이름이다.
 */
function scopeName(
    scope,
) {
    let name = AgentAccount.CURRENT_NAME;
    if (scope?.id) name = scope.label;
    else if (scope?.custom) name = AgentAccount.CUSTOM_NAME;
    else if (scope?.linked?.label) name = scope.linked.label;
    return name;
}

module.exports = { AgentAccount, parseEnviron, normalizePath, attributeAccount, runningScope, scopeName };
