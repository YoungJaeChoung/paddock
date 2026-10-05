/** Resolves registered account usage separately from the default CLI environment. */
const fs = require('node:fs');
const path = require('node:path');
const { AccountProfile } = require('./account-profiles');
const { CodexUsage } = require('./codex-usage');
const usage = require('./usage-model');
const { assertPlainPath, usageError, readJson, codexUsage } = require('./usage-files');
const { applyStatusLine, writeJson } = require('./claude-usage-settings');

/** Converts only Linux absolute paths on the selected distribution, keeping its home mapping intact. */
function wslFileMapping(
    information,
    distribution,
) {
    const linuxHome = information.linuxHome;
    const windowsHome = information.home;
    if (!information.ready || typeof linuxHome !== 'string' || !linuxHome.startsWith('/') || /[\u0000-\u001f]/.test(linuxHome) || typeof windowsHome !== 'string') {
        throw usageError('The account WSL distribution is unavailable. Start it and try again.', 503);
    }
    const suffix = linuxHome.replace(/\//g, '\\');
    const root = windowsHome.slice(0, windowsHome.length - suffix.length);
    const match = /^\\\\(?:wsl\.localhost|wsl\$)\\([^\\]+)$/i.exec(root);
    if (!windowsHome.endsWith(suffix) || !match || match[1].toLowerCase() !== distribution.toLowerCase()) {
        throw usageError('The account WSL folder does not belong to its saved distribution.', 503);
    }
    const toLocal = file => {
        if (typeof file !== 'string' || !file.startsWith('/') || /[\u0000-\u001f]/.test(file)) throw usageError('The WSL usage path is invalid.');
        const normalized = path.posix.normalize(file);
        return `${root}${normalized.replace(/\//g, '\\')}`;
    };
    return { linuxHome, toLocal };
}

/**
 * Claude 로그인 계정의 고유 번호(oauthAccount.accountUuid). 인증 토큰이 아니라 Claude Code가 남기는 계정 정보만 읽는다.
 * 파일이 없거나 Claude가 쓰는 도중이라 읽지 못하면 null이다.
 */
function claudeLogin(
    file,
) {
    let login = null;
    try {
        const accountUuid = JSON.parse(fs.readFileSync(file, 'utf8')).oauthAccount?.accountUuid;
        if (typeof accountUuid === 'string' && accountUuid) login = accountUuid;
    } catch {
        login = null;
    }
    return login;
}

/**
 * Codex 로그인 계정을 가리키는 비밀이 아닌 식별자. auth.json을 파싱한 값을 받아 `ChatGPT 계정 id[/사용자 id]` 문자열을 돌려준다.
 * 기본 CLI 폴더와 등록 계정 폴더가 같은 로그인인지 대조하려고 존재한다. 토큰 자체는 결과에 들어가지 않는다.
 * 계정 id는 `tokens.account_id`, 없으면 id_token 본문의 `chatgpt_account_id`다. 팀 작업 공간은 사용자마다 계정 id가 같으므로
 * id_token 본문에 사용자 id(`chatgpt_user_id`·`user_id`·`sub`)가 있으면 함께 붙인다 — 한쪽에만 있으면 서로 다른 값이 되어 연결되지 않는다.
 * API 키 로그인처럼 계정 id가 없거나 형식이 다르면 null이다.
 *
 * Examples
 * --------
 * | auth.json                                                      | 결과            |
 * | -------------------------------------------------------------- | --------------- |
 * | `{ tokens: { account_id: 'acc-1' } }`                          | `'acc-1'`       |
 * | `{ tokens: { account_id: 'acc-1', id_token: <sub 'u-1'> } }`   | `'acc-1/u-1'`   |
 * | `{ tokens: { id_token: <chatgpt_account_id 'acc-2'> } }`       | `'acc-2'`       |
 * | `{ OPENAI_API_KEY: 'sk-…' }`                                   | `null`          |
 */
function codexLoginId(
    auth,
) {
    const tokens = auth?.tokens;
    let claims = null;
    try {
        // JWT의 두 번째 조각(본문)만 디코딩한다. 서명 검증은 하지 않는다 — 같은 로그인인지 대조하는 용도일 뿐 인증에 쓰지 않는다.
        const payload = typeof tokens?.id_token === 'string' ? tokens.id_token.split('.')[1] : '';
        claims = payload ? JSON.parse(Buffer.from(payload, 'base64url').toString('utf8')) : null;
    } catch {
        claims = null;
    }
    const auth0 = claims?.['https://api.openai.com/auth'];
    const accountId = [tokens?.account_id, auth0?.chatgpt_account_id].find(value => typeof value === 'string' && value);
    const userId = [auth0?.chatgpt_user_id, auth0?.user_id, claims?.sub].find(value => typeof value === 'string' && value);
    let login = null;
    if (accountId) login = userId ? `${accountId}/${userId}` : accountId;
    return login;
}

/** Codex 설정 폴더의 auth.json에서 읽은 로그인 식별자(codexLoginId). 파일이 없거나 쓰는 도중이라 읽지 못하면 null이다. */
function codexLogin(
    file,
) {
    let login = null;
    try {
        login = codexLoginId(JSON.parse(fs.readFileSync(file, 'utf8')));
    } catch {
        login = null;
    }
    return login;
}

/**
 * 기본 CLI의 로그인과 같은 로그인을 쓰는 등록 계정. 정확히 하나일 때만 그 계정이고, 없거나 둘 이상(어느 쪽인지 모호)이면 null이다.
 * 기본 CLI를 그 계정으로 취급해도 되는지(이름·사용량 행을 합칠지) 정하는 마지막 판정이다.
 *
 * Parameters
 * ----------
 * login : 기본 CLI의 로그인 식별자(claudeLogin·codexLogin). 빈 값이면 null을 돌려준다.
 * candidates : 등록 계정과 그 폴더의 로그인 식별자 `[{ profile, login }]`. 로그인을 읽지 못한 계정은 login이 null이다.
 *
 * Examples
 * --------
 * | login | candidates의 login         | 결과   |
 * | ----- | -------------------------- | ------ |
 * | `a`   | A=`a`, B=`b`               | A      |
 * | `a`   | A=`a`, B=`a`               | null   |
 * | `a`   | A=`b`, B=null              | null   |
 * | null  | A=null                     | null   |
 */
function uniqueLoginMatch(
    login,
    candidates,
) {
    const matches = login ? candidates.filter(candidate => candidate.login === login) : [];
    return matches.length === 1 ? matches[0].profile : null;
}

/**
 * 실행 환경의 계정 변수(parseEnviron 결과)로 기본 CLI의 로그인 정보 파일 경로를 정한다(그 실행 환경 기준 경로).
 * Claude는 CLAUDE_CONFIG_DIR가 있으면 그 안의 `.claude.json`, 없으면 `$HOME/.claude.json`이고, Codex는 `CODEX_HOME`(없으면 `$HOME/.codex`)의 `auth.json`이다.
 * 경로를 정할 수 없으면 null이다.
 */
function defaultLoginFile(
    provider,
    env,
    join = path.join,
) {
    let file = null;
    if (provider === 'claude' && (env?.CLAUDE_CONFIG_DIR || env?.HOME)) file = join(env.CLAUDE_CONFIG_DIR || env.HOME, AccountUsage.LOGIN_FILES.claude);
    else if (provider === 'codex' && (env?.CODEX_HOME || env?.HOME)) file = join(env.CODEX_HOME || join(env.HOME, '.codex'), AccountUsage.LOGIN_FILES.codex);
    return file;
}

/** 로그인 정보 파일에서 도구별 로그인 식별자를 읽는다. 다른 도구 이름이면 null이다. */
function readLogin(
    provider,
    file,
) {
    let login = null;
    if (provider === 'claude') login = claudeLogin(file);
    else if (provider === 'codex') login = codexLogin(file);
    return login;
}

class AccountUsage {
    /** 설정 폴더 안의 로그인 정보 파일. CLAUDE_CONFIG_DIR를 지정한 Claude는 .claude.json도 그 폴더 안에 둔다. */
    static LOGIN_FILES = Object.freeze({ claude: '.claude.json', codex: 'auth.json' });
    /** 등록 계정의 로그인 식별자를 다시 읽지 않고 쓰는 시간. 터미널 상태를 몇 초마다 묻는 요청이 매번 계정 폴더를 읽지 않게 한다. */
    static LOGIN_CACHE_MS = 10000;

    constructor(
        { accounts, readWslInfo, sourceScript, commandForNative, liveCodexUsage = new CodexUsage() },
    ) {
        this.accounts = accounts;
        this.readWslInfo = readWslInfo;
        this.sourceScript = sourceScript;
        this.commandForNative = commandForNative;
        this.liveCodexUsage = liveCodexUsage;
        this.pendingWrites = Promise.resolve();
        /** 등록 계정 id별 로그인 식별자 읽기 결과 `{ readAt, pending }`. savedLogins가 LOGIN_CACHE_MS 동안 다시 쓴다. */
        this.savedLoginCache = new Map();
    }

    /** Reads account metadata and paths without creating files or preparing a terminal. */
    async resolve(
        accountId,
    ) {
        if (typeof accountId !== 'string' || !AccountProfile.ID.test(accountId)) throw usageError('accountId must identify a saved account.', 400);
        const profile = (await this.accounts.list()).find(item => item.id === accountId);
        if (!profile) throw usageError('This account is no longer available. Refresh the account list.', 404);
        let configDir;
        let toLocal = file => file;
        let node = null;
        let managedDirectories;
        if (profile.runtime === 'wsl') {
            const information = await this.readWslInfo(profile.wslDistribution);
            const mapping = wslFileMapping(information, profile.wslDistribution);
            toLocal = mapping.toLocal;
            node = information.node || null;
            const profileRoot = path.posix.join(mapping.linuxHome, '.paddock', 'agent-profiles');
            configDir = path.posix.join(profileRoot, profile.id, profile.provider);
            managedDirectories = [path.posix.join(mapping.linuxHome, '.paddock'), profileRoot, path.posix.join(profileRoot, profile.id), configDir];
        } else {
            const profileRoot = path.join(this.accounts.configDirectory, 'agent-profiles');
            configDir = path.join(profileRoot, profile.id, profile.provider);
            managedDirectories = [this.accounts.configDirectory, profileRoot, path.join(profileRoot, profile.id), configDir];
        }
        for (const directory of managedDirectories) assertPlainPath(toLocal(directory), 'directory');
        const join = profile.runtime === 'wsl' ? path.posix.join : path.join;
        const usageDirectory = join(configDir, 'paddock-usage');
        const settingsPath = join(configDir, 'settings.json');
        const sessionsDirectory = join(configDir, 'sessions');
        assertPlainPath(toLocal(usageDirectory), 'directory');
        assertPlainPath(toLocal(sessionsDirectory), 'directory');
        return { profile, configDir, usageDirectory, settingsPath, sessionsDirectory, toLocal, node, join };
    }

    async read(
        accountId,
    ) {
        const scope = await this.resolve(accountId);
        const result = { claude: { state: 'unset', windows: [], updatedAt: null }, codex: { windows: [], updatedAt: null } };
        if (scope.profile.provider === 'claude') {
            const stateFile = scope.toLocal(scope.join(scope.usageDirectory, 'state.json'));
            const usageFile = scope.toLocal(scope.join(scope.usageDirectory, 'claude.json'));
            for (const file of [stateFile, usageFile]) assertPlainPath(file, 'file');
            const record = readJson(usageFile, null);
            result.claude = { state: readJson(stateFile, {}).claude || 'unset', windows: record ? usage.claudeWindows(record) : [], updatedAt: record?.updated_at ?? null };
        } else {
            let liveError = null;
            try {
                result.codex = await this.liveCodexUsage.read(scope);
            } catch (error) {
                liveError = error;
            }
            if (!result.codex.windows.length) {
                const recorded = codexUsage([scope.toLocal(scope.sessionsDirectory)]);
                // 실시간 값 대신 세션 기록을 쓰면 표시해, 화면이 지금 값처럼 보이지 않게 한다.
                if (recorded.windows.length) result.codex = { ...recorded, recorded: true };
                else if (liveError) throw liveError;
            }
        }
        return result;
    }

    /**
     * 도구·실행 환경이 맞는 등록 계정과 그 폴더의 로그인 식별자 `[{ profile, login }]`. 계정마다 몇 초 동안은 지난 읽기 결과를 다시 쓴다.
     * 계정 목록은 매번 새로 읽는다. 그래서 같은 로그인의 계정을 막 추가하면 바로 모호해지고, 지운 계정은 바로 대조에서 빠진다.
     * 캐시 때문에 늦어지는 것은 계정 폴더에서 다시 로그인한 경우뿐이다(최대 LOGIN_CACHE_MS).
     * runtime이 없으면 실행 환경을 가리지 않고, WSL은 wslDistribution을 알 때만 배포판까지 비교한다.
     * 로그인 정보를 확인할 수 없는 계정(WSL이 꺼져 있거나 폴더가 바뀐 계정)은 login이 null이다.
     */
    async savedLogins(
        { provider, runtime, wslDistribution = '' },
    ) {
        const listed = await this.accounts.list();
        const profiles = listed.filter(item => item.provider === provider && (!runtime || item.runtime === runtime)
            && (runtime !== 'wsl' || !wslDistribution || String(item.wslDistribution || '').toLowerCase() === String(wslDistribution).toLowerCase()));
        // 목록에서 빠진 계정의 캐시는 버린다. 같은 id가 다시 생기는 일은 없지만, 쌓이지 않게 한다.
        for (const id of this.savedLoginCache.keys()) if (!listed.some(item => item.id === id)) this.savedLoginCache.delete(id);
        return Promise.all(profiles.map(profile => {
            const cached = this.savedLoginCache.get(profile.id);
            let pending = cached && Date.now() - cached.readAt < AccountUsage.LOGIN_CACHE_MS ? cached.pending : null;
            if (!pending) {
                pending = (async () => {
                    let login = null;
                    try {
                        const scope = await this.resolve(profile.id);
                        const file = scope.toLocal(scope.join(scope.configDir, AccountUsage.LOGIN_FILES[provider]));
                        assertPlainPath(file, 'file');
                        login = readLogin(provider, file);
                    } catch {
                        // WSL이 꺼져 있거나 폴더가 바뀐 계정은 대조하지 않는다.
                        login = null;
                    }
                    return login;
                })();
                this.savedLoginCache.set(profile.id, { readAt: Date.now(), pending });
            }
            return pending.then(login => ({ profile, login }));
        }));
    }

    /**
     * 기본 CLI의 로그인과 같은 로그인을 쓰는 등록 계정(uniqueLoginMatch). 정확히 하나일 때만 그 계정이고, 아니면 null이다.
     * 기본 CLI를 그 계정으로 보여 주고(이름·사용량 행) 같은 계정의 사용량을 한 값으로 맞추려고 존재한다.
     * runtime·wslDistribution을 주면 그 실행 환경의 등록 계정만 대조한다.
     */
    async matchLogin(
        { provider, login, runtime, wslDistribution = '' },
    ) {
        return login ? uniqueLoginMatch(login, await this.savedLogins({ provider, runtime, wslDistribution })) : null;
    }

    /**
     * 기본 CLI와 같은 계정으로 로그인한 등록 Claude 계정과 그 계정의 사용량 기록을 찾는다.
     * 기본 Claude 터미널에 등록 계정 이름을 붙이고, 두 곳 중 더 최근 기록을 쓰게 하려고 존재한다.
     * 로그인 정보를 확인할 수 없는 계정은 건너뛰고, 일치하는 계정이 없거나 둘 이상이면(어느 쪽인지 모호) null이다.
     * where에 runtime·wslDistribution을 주면 그 실행 환경의 등록 계정만 대조한다.
     */
    async matchClaudeLogin(
        login,
        where = {},
    ) {
        const profile = await this.matchLogin({ provider: 'claude', login, ...where });
        return profile ? this.claudeRecord(profile.id) : null;
    }

    /**
     * 등록 Claude 계정과 그 계정 폴더의 사용량 기록 `{ profile, record }`. 기록이 아직 없으면 record가 null이다.
     * 기본 Claude가 이 계정과 같은 로그인으로 확인됐을 때, 두 기록 중 더 최근 것을 고르려고 존재한다.
     * 사이에 계정이 지워졌거나 폴더를 읽을 수 없으면 null이다.
     */
    async claudeRecord(
        accountId,
    ) {
        let match = null;
        try {
            const scope = await this.resolve(accountId);
            const usageFile = scope.toLocal(scope.join(scope.usageDirectory, 'claude.json'));
            assertPlainPath(usageFile, 'file');
            match = { profile: scope.profile, record: readJson(usageFile, null) };
        } catch {
            // 사이에 계정이 지워졌거나 기록을 읽을 수 없으면 연결하지 않는다.
            match = null;
        }
        return match;
    }

    /** Serializes setting changes so checkbox updates cannot overwrite another account's saved command. */
    async setClaude(
        accountId,
        enabled,
        isAutomatic = false,
    ) {
        const operation = this.pendingWrites.then(async () => {
            const scope = await this.resolve(accountId);
            if (scope.profile.provider !== 'claude') throw usageError('Claude usage settings require a Claude account.', 400);
            const script = scope.join(scope.usageDirectory, 'claude-statusline.cjs');
            const usageFile = scope.join(scope.usageDirectory, 'claude.json');
            const previousFile = scope.join(scope.usageDirectory, 'previous-statusline.json');
            const stateFile = scope.join(scope.usageDirectory, 'state.json');
            for (const file of [scope.settingsPath, `${scope.settingsPath}.paddock-backup`, script, usageFile, previousFile, stateFile]) assertPlainPath(scope.toLocal(file), 'file');
            let command = scope.profile.runtime === 'wsl'
                ? scope.node && [scope.node, script, usageFile, previousFile].map(file => usage.quoteStatusLineArgument(file)).join(' ')
                : this.commandForNative(script, usageFile, previousFile);
            if (enabled && !command) throw usageError('Node.js is needed to show Claude usage in this account. Install it in the selected environment and try again.', 503);
            if (!enabled) {
                const current = readJson(scope.toLocal(scope.settingsPath), {}).statusLine?.command;
                const files = usage.paddockStatusLineFiles(current);
                if (files?.script === script && files?.previousFile === previousFile && files?.usageFile === usageFile) command = current;
            }
            if (!fs.existsSync(scope.toLocal(scope.usageDirectory))) fs.mkdirSync(scope.toLocal(scope.usageDirectory), { mode: 0o700 });
            if (enabled) fs.copyFileSync(this.sourceScript, scope.toLocal(script));
            const state = applyStatusLine({ settingsPath: scope.toLocal(scope.settingsPath), previousFile: scope.toLocal(previousFile), command, enabled, isAutomatic, toLocal: scope.toLocal, scoped: true });
            writeJson(scope.toLocal(stateFile), { claude: state });
            return { state };
        });
        this.pendingWrites = operation.catch(() => {});
        return operation;
    }
}

module.exports = { AccountUsage, wslFileMapping, claudeLogin, codexLoginId, codexLogin, uniqueLoginMatch, defaultLoginFile, readLogin };
