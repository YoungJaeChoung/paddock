Object.defineProperty(exports, "__esModule", { value: true });
// 상태 줄 게이지(메모리·AI 사용량)에 쓰는 값을 백엔드에서 읽어 준다. 원격 창이면 그 호스트의 값이다.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFile, spawn, spawnSync } = require('node:child_process');
const express = require('express');
const { ContainerModule, injectable, decorate } = require('@theia/core/shared/inversify');
const { BackendApplicationContribution } = require('@theia/core/lib/node/backend-application');
const { PluginHostEnvironmentVariable } = require('@theia/plugin-ext/lib/common/plugin-protocol');
const { ConnectionHandler, RpcConnectionHandler } = require('@theia/core/lib/common/messaging');
const { AccountProfile, AccountProfiles } = require('./account-profiles');
const { AccountUsage, wslFileMapping, defaultLoginFile, readLogin } = require('./account-usage');
const { CodexUsage } = require('./codex-usage');
const { AccountSessions } = require('./account-session');
const { readJson, usageError } = require('./usage-files');
const { applyStatusLine, settingsTarget } = require('./claude-usage-settings');
const { memoryPercent, macUsedMemory } = require('./work-model');
const usage = require('./usage-model');
const agent = require('./agent-model');
const { WorkPresenceRegistry } = require('./work-presence');
const wsl = require('./wsl-terminals');
const windowsProcesses = require('./windows-processes');
const { attributeAccount, parseEnviron } = require('./agent-account');
const { trustWslRepositories } = require('./git-environment');

const accounts = new AccountProfiles();

function usageDirectory() {
    const directory = path.join(process.env.THEIA_CONFIG_DIR || path.join(os.homedir(), '.paddock', 'config'), 'usage');
    fs.mkdirSync(directory, { recursive: true });
    return directory;
}

function claudeSettingsPath() {
    return path.join(process.env.CLAUDE_CONFIG_DIR || path.join(os.homedir(), '.claude'), 'settings.json');
}

/**
 * 셸 프로세스의 지금 앞쪽 프로그램(터미널에서 실행 중인 명령)의 명령줄과 계정 환경: `{ argv, env }`. 셸만 떠 있으면 셸의 명령줄이다.
 * env는 그 프로세스의 CODEX_HOME·CLAUDE_CONFIG_DIR·HOME만 담고, 읽을 수 없으면 null(모름)이다.
 * Linux는 /proc, macOS는 ps로 읽는다(macOS는 env를 읽지 않는다). 명령줄을 읽을 수 없으면 null.
 * Windows는 WindowsProcessList와 windows-processes.js가 맡는다.
 */
function foregroundProcess(
    shellPid,
) {
    let result = null;
    try {
        if (process.platform === 'linux') {
            // stat의 8번째 칸이 터미널 앞쪽 프로세스 그룹. 프로그램 이름에 공백·괄호가 있을 수 있어 마지막 ')' 뒤부터 센다.
            const stat = fs.readFileSync(`/proc/${shellPid}/stat`, 'utf8');
            const foreground = Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[5]);
            const pid = foreground > 0 ? foreground : shellPid;
            const argv = fs.readFileSync(`/proc/${pid}/cmdline`, 'utf8').split('\0').filter(Boolean);
            let env = null;
            try {
                env = parseEnviron(fs.readFileSync(`/proc/${pid}/environ`, 'utf8'));
            } catch {
                // 다른 사용자로 실행된 프로그램(sudo 등)은 환경을 읽을 수 없다. 계정은 모름으로 둔다.
                env = null;
            }
            result = { argv, env };
        } else if (process.platform === 'darwin') {
            const group = spawnSync('ps', ['-o', 'tpgid=', '-p', String(shellPid)], { encoding: 'utf8' }).stdout.trim();
            const pid = Number(group) > 0 ? group : String(shellPid);
            const args = spawnSync('ps', ['-o', 'args=', '-p', pid], { encoding: 'utf8' }).stdout.trim();
            result = args ? { argv: args.split(/\s+/), env: null } : null;
        }
    } catch {
        result = null;
    }
    return result;
}

/** 실제 경로. 링크를 따라갈 수 없으면 입력 경로를 그대로 쓴다. */
function nativeRealPath(
    file,
) {
    let result = file;
    try {
        result = fs.realpathSync.native(file);
    } catch {
        result = file;
    }
    return result;
}

/** Windows 경로 `\\wsl.localhost\<배포판>\…`(또는 `\\wsl$\…`)의 배포판 이름. WSL 경로가 아니면 빈 문자열이다. */
function wslDistributionOf(
    windowsPath,
) {
    return /^\\\\(?:wsl\.localhost|wsl\$)\\([^\\]+)/i.exec(String(windowsPath || ''))?.[1] || '';
}

/**
 * 계정 판정 결과를 화면에 보낼 모양으로 줄인다. 폴더 경로는 보내지 않고 계정의 공개 항목만 남긴다.
 * 기본 CLI(`default`)에 profile이 있으면 그 로그인이 등록 계정 하나와 같다고 확인된 경우다(같은 모양으로 줄인다).
 */
function publicAccount(
    found,
) {
    let result = found;
    if (found?.profile) {
        const { id, provider, label, runtime, wslDistribution } = found.profile;
        result = { kind: found.kind, profile: { id, provider, label, runtime, ...(wslDistribution ? { wslDistribution } : {}) } };
    }
    return result;
}

/**
 * Paddock 상태 줄을 실행할 명령. PATH의 node를 쓰고, 없으면(macOS·Linux) 앱의 Electron을 Node로 돌린다.
 * Windows에서 node가 없으면 명령을 만들지 못한다(null).
 */
/**
 * Windows 프로세스 목록(`{ pid, parentPid, commandLine, createdAt }`)을 읽는다.
 * PowerShell을 매번 띄우면 시작에만 0.5초쯤 걸려, 하나를 띄워 두고 표준 입력으로 요청을 보낸다. 요청은 한 번에 하나씩 처리한다.
 * 응답이 늦으면 그 PowerShell을 끝내고 다음 요청에서 새로 띄운다. 앱이 끝나면 표준 입력이 닫혀 PowerShell도 끝난다.
 */
class WindowsProcessList {
    static END = '__PADDOCK_PROCESSES_END__';
    static SCRIPT = [
        '$list = @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,CommandLine,CreationDate | ForEach-Object {',
        '[pscustomobject]@{ pid = $_.ProcessId; parentPid = $_.ParentProcessId; commandLine = $_.CommandLine; createdAt = $_.CreationDate.Ticks; startIdentity = $_.CreationDate.ToFileTimeUtc().ToString() } });',
        '[Console]::Out.WriteLine((ConvertTo-Json -Compress -InputObject $list));',
        "[Console]::Out.WriteLine('__PADDOCK_PROCESSES_END__')",
    ].join(' ');

    constructor() {
        this.child = null;
        this.buffer = '';
        this.pending = null;
        this.queue = Promise.resolve();
    }

    read() {
        const result = this.queue.then(() => this.readOnce());
        this.queue = result.catch(() => undefined);
        return result;
    }

    readOnce() {
        if (!this.child) {
            // 명령줄의 한글이 깨지지 않게 출력은 UTF-8로 받는다.
            this.child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-NoLogo', '-Command', '-'], { windowsHide: true });
            this.child.stdout.setEncoding('utf8');
            this.child.stdout.on('data', chunk => this.receive(chunk));
            this.child.once('exit', () => {
                this.child = null;
                this.pending?.reject(new Error('PowerShell ended'));
            });
            this.child.stdin.write('[Console]::OutputEncoding = [Text.Encoding]::UTF8\n');
        }
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => this.child?.kill(), 5000);
            this.pending = {
                resolve: (value) => { clearTimeout(timer); this.pending = null; resolve(value); },
                reject: (error) => { clearTimeout(timer); this.pending = null; reject(error); },
            };
            this.child.stdin.write(`${WindowsProcessList.SCRIPT}\n`);
        });
    }

    receive(
        chunk,
    ) {
        this.buffer += chunk;
        const end = this.buffer.indexOf(WindowsProcessList.END);
        if (end >= 0) {
            const text = this.buffer.slice(0, end);
            this.buffer = this.buffer.slice(end + WindowsProcessList.END.length);
            let list = [];
            try {
                list = JSON.parse(text.trim() || '[]');
            } catch {
                list = [];
            }
            this.pending?.resolve(Array.isArray(list) ? list : [list]);
        }
    }
}

/** macOS에서 캐시를 뺀 사용 메모리(바이트). `vm_stat`을 실행하지 못하거나 출력을 해석하지 못하면 null이라 호출부가 os.freemem()으로 돌아간다. */
function readMacUsedMemory() {
    return new Promise(resolve => execFile('vm_stat', (error, stdout) => resolve(error ? null : macUsedMemory(stdout))));
}

const wslInfoPromises = new Map();

/**
 * Windows에 설치된 지정 WSL 배포판의 정보: `{ ready, home, linuxHome, node }`. 생략하면 기본 배포판이다.
 * home은 홈 폴더의 Windows 경로(\\wsl.localhost\…), linuxHome은 Linux 경로, node는 WSL 안 node 실행 파일(없으면 '')이다.
 * Windows가 아니거나 배포판이 없으면 ready가 false다. 성공한 결과만 기억하고, 실패하면 다음 호출에서 다시 읽는다.
 */
function readWslInfo(
    distribution = '',
) {
    if (!wslInfoPromises.has(distribution)) {
        const empty = { ready: false, home: '', linuxHome: '', node: '', distributions: [] };
        const pending = process.platform !== 'win32' ? Promise.resolve(empty) : new Promise((resolve) => {
            // wsl.exe -l은 UTF-16으로 배포판 이름을 한 줄씩 출력한다. 배포판이 없으면 오류로 끝난다.
            execFile('wsl.exe', ['-l', '-q'], { encoding: 'buffer', timeout: 5000, windowsHide: true }, (error, stdout) => {
                const names = error ? [] : stdout.toString('utf16le').split(/\r?\n/).map(name => name.replace(/\0/g, '').trim()).filter(Boolean);
                if (!names.length || (distribution && !names.some(name => name.toLowerCase() === distribution.toLowerCase()))) {
                    resolve(empty);
                } else {
                    // nvm처럼 대화형 셸 설정에서 PATH를 잡는 node도 찾도록 bash -i로 읽는다. 설정 파일이 찍는 글은 앞쪽 줄이라 끝 세 줄만 쓴다.
                    // WSL을 켜는 데 몇 초 걸릴 수 있다.
                    const probe = 'printf "\\n%s\\n%s\\n%s" "$(wslpath -w "$HOME")" "$HOME" "$(command -v node)"';
                    const selectedDistribution = distribution ? ['--distribution', distribution] : [];
                    execFile('wsl.exe', [...selectedDistribution, '-e', 'bash', '-ic', probe], { encoding: 'utf8', timeout: 20000, windowsHide: true }, (probeError, output) => {
                        const [home = '', linuxHome = '', node = ''] = (probeError ? '' : output).replace(/\r/g, '').split('\n').slice(-3);
                        resolve({ ready: true, home: home.trim(), linuxHome: linuxHome.trim(), node: node.trim(), distributions: names });
                    });
                }
            });
        });
        wslInfoPromises.set(distribution, pending);
        pending.then((info) => {
            if (process.platform === 'win32' && (!info.ready || !info.home)) wslInfoPromises.delete(distribution);
        });
    }
    return wslInfoPromises.get(distribution);
}

/**
 * WSL 안 Linux 경로의 링크를 모두 따라간 Linux 경로. 기본 배포판에서 `readlink -f`로 읽고, 읽지 못하면 받은 경로를 돌려준다.
 * Windows 쪽에서 \\wsl.localhost 경로로는 WSL 심볼릭 링크를 따라가지 못하기 때문이다(lstat이 EISDIR, 읽기가 ENOENT로 실패한다).
 */
function resolveWslLink(
    linuxPath,
) {
    return new Promise((resolve) => {
        execFile('wsl.exe', ['-e', 'readlink', '-f', '--', linuxPath], { encoding: 'utf8', timeout: 10000, windowsHide: true }, (error, output) => {
            const resolved = error ? '' : output.replace(/\r/g, '').trim();
            resolve(resolved.startsWith('/') ? resolved : linuxPath);
        });
    });
}

function statusLineCommand(
    script,
    usageFile,
    previousFile,
) {
    const finder = process.platform === 'win32' ? 'where' : 'which';
    const found = spawnSync(finder, ['node'], { encoding: 'utf8' }).stdout?.split(/\r?\n/)[0]?.trim();
    const quote = (value) => usage.quoteStatusLineArgument(value, process.platform === 'win32');
    let runner = null;
    if (found) runner = quote(found);
    else if (process.platform !== 'win32') runner = `ELECTRON_RUN_AS_NODE=1 ${quote(process.execPath)}`;
    return runner ? `${runner} ${quote(script)} ${quote(usageFile)} ${quote(previousFile)}` : null;
}

const windowsProcessList = new WindowsProcessList();
const accountSessions = new AccountSessions({ accounts, readWslInfo, readWindowsProcesses: () => windowsProcessList.read() });

/**
 * `GET /paddock/memory` → `{ percent, total, free }`.
 * `GET /paddock/usage[?accountId=…|?provider=claude|codex]` → `{ claude: { state, windows, updatedAt }, codex: { windows, updatedAt } }`.
 * accountId가 있으면 등록된 그 계정만 읽고, 다른 도구의 값은 빈 목록이다. 생략하면 기본 CLI 환경을 읽는다.
 * 기본 Claude의 로그인이 등록 계정과 같으면 `claude.account`에 그 계정의 `{ id, label }`을 붙인다. 다르거나 확인할 수 없으면 빠진다.
 * 기본 Codex도 같다(`codex.account`): 로그인한 기본 환경(이 컴퓨터·WSL)이 모두 같은 등록 계정 하나와 맞을 때만 붙는다.
 * 대조는 같은 실행 환경(WSL은 같은 배포판)의 등록 계정끼리만 하고, 같은 로그인의 등록 계정이 둘 이상이면 붙이지 않는다.
 * provider를 지정하면 다른 도구의 설정이나 사용량을 읽지 않는다. 둘 다 생략하면 두 도구를 읽는다.
 * `GET /paddock/foreground?pids=1,2` → `{ "1": { program: "claude", account }, "2": { program: "bash", account: null } }`
 *   셸 pid별 실행 중 프로그램 이름(모르면 program이 null)과, claude·codex면 그 프로세스 환경으로 정한 계정(agent-account.js의 attributeAccount 결과).
 *   account는 `{ kind: 'default' }`·`{ kind: 'profile', profile: { id, provider, label, runtime } }`·`{ kind: 'custom' }`이고,
 *   기본 CLI의 로그인이 같은 실행 환경의 등록 계정 하나와 같으면 `{ kind: 'default', profile }`로 그 계정을 함께 보낸다(WSL 터미널도 같다).
 *   로그인 정보 파일의 비밀이 아닌 식별자만 대조하며 토큰이나 경로는 응답에 넣지 않는다.
 *   에이전트가 아니거나 환경을 읽지 못하면(Windows·macOS 포함) null이다. Windows는 프로세스 목록으로 프로그램을 찾는다.
 * `GET /paddock/wsl-terminals` → `{ [터미널 id]: { cwd, program, account } }` Windows 앱의 WSL 터미널별 현재 폴더(Windows 경로)·실행 중 프로그램·계정.
 *   Windows가 아니거나 WSL을 읽지 못하면 `{}`다.
 * `GET /paddock/wsl-ready` → `{ ready, home }` 이 Windows에 WSL 배포판이 하나라도 설치돼 있는지와 기본 배포판의 홈 폴더(Windows 경로).
 *   wsl.exe만 있고 배포판이 없으면 `{ ready: false, home: '' }`다.
 * `POST /paddock/usage/claude?enabled=true|false[&automatic=true][&accountId=…]` → 해당 Claude Code 설정에 Paddock 상태 줄을 넣거나 원래대로 되돌린다.
 *   automatic이면(처음 실행의 자동 켜기) 다른 Paddock 설정 폴더의 상태 줄이 살아 있을 때 바꾸지 않고 `{ state: 'unset' }`을 돌려준다.
 * Claude의 `state`는 'unset'(아직 정한 적 없음)·'on'·'off'다.
 */
class PaddockStatusRoutes {
    constructor() {
        this.workPresence = new WorkPresenceRegistry();
        this.windowsProcesses = windowsProcessList;
        this.liveCodexUsage = new CodexUsage();
        this.accountUsage = new AccountUsage({
            accounts, readWslInfo, liveCodexUsage: this.liveCodexUsage,
            sourceScript: path.join(process.env.THEIA_APP_PROJECT_PATH || process.cwd(), 'paddock-shell', 'claude-statusline.cjs'),
            commandForNative: statusLineCommand,
        });
    }

    configure(
        app,
    ) {
        // 모든 Paddock 창이 한 백엔드에 자신의 목록 정보만 보내고 다른 창의 목록을 받는다.
        app.post('/paddock/work-presence', express.json({ limit: '64kb' }), (request, response) => {
            const { windowId, folders, terminals } = request.body || {};
            if (typeof windowId !== 'string' || !windowId) {
                response.status(400).json({ error: 'windowId is required' });
            } else {
                response.json(this.workPresence.update(windowId, { folders, terminals }));
            }
        });
        app.delete('/paddock/work-presence', (request, response) => {
            if (typeof request.query.windowId === 'string') this.workPresence.remove(request.query.windowId);
            response.status(204).end();
        });
        app.get('/paddock/memory', async (request, response) => {
            const total = os.totalmem();
            // Linux(MemAvailable)·Windows(사용 가능 메모리)의 os.freemem()은 캐시를 이미 가용으로 센다. macOS만 캐시를 뺀 사용량을 따로 읽는다.
            const used = process.platform === 'darwin' ? await readMacUsedMemory() : null;
            const free = used === null ? os.freemem() : Math.max(0, total - used);
            response.json({ percent: memoryPercent(total, free), total, free });
        });
        app.get('/paddock/foreground', async (request, response) => {
            const names = {};
            const pids = String(request.query.pids || '').split(',').filter(value => /^\d+$/.test(value));
            // Windows에는 터미널의 앞쪽 프로세스가 없어 프로세스 목록에서 셸의 자식을 찾는다. 목록을 못 읽으면 모두 null이다.
            const processes = process.platform === 'win32' && pids.length ? await this.windowsProcesses.read().catch(() => []) : [];
            const foregrounds = pids.map(pid => [pid, process.platform === 'win32'
                ? { argv: windowsProcesses.foregroundArgv(processes, Number(pid)), env: null } : foregroundProcess(Number(pid))]);
            // 계정 목록은 에이전트의 환경을 읽은 경우에만 한 번 읽는다. 읽지 못하면 모든 계정 폴더를 미등록으로 보지 않도록 판정을 건너뛴다.
            const profiles = foregrounds.some(([, item]) => item?.env && agent.isAgent(agent.programName(item.argv || []))) ? await accounts.list().catch(() => null) : null;
            for (const [pid, item] of foregrounds) {
                const program = item?.argv ? agent.programName(item.argv) : null;
                const found = item?.env && profiles ? attributeAccount({
                    provider: program, runtime: 'native', env: item.env, profiles,
                    profileDirectory: profile => path.join(accounts.configDirectory, 'agent-profiles', profile.id, profile.provider),
                    // 앱이 CODEX_HOME·CLAUDE_CONFIG_DIR을 지정해 실행됐으면 일반 터미널도 그 폴더를 물려받는다. 그 폴더가 이 앱의 기본 CLI다.
                    defaultDirectories: [program === 'claude' ? accounts.claudeDirectory : accounts.codexDirectory], realPath: nativeRealPath,
                }) : null;
                // 기본 CLI의 로그인이 이 실행 환경의 등록 계정 하나와 같으면 그 계정을 함께 보낸다. 화면은 기본 CLI를 그 계정 이름으로 보인다.
                const linked = found?.kind === 'default'
                    ? await this.accountUsage.matchLogin({ provider: program, runtime: 'native', login: readLogin(program, defaultLoginFile(program, item.env)) }).catch(() => null) : null;
                names[pid] = { program, account: publicAccount(linked ? { ...found, profile: linked } : found) };
            }
            response.json(names);
        });
        app.get('/paddock/wsl-terminals', (request, response) => {
            if (process.platform !== 'win32') {
                response.json({});
            } else {
                // WSL이 꺼져 있으면 켜는 데 몇 초 걸린다. 늦으면 이번 주기는 비우고 다음 주기에 다시 읽는다.
                execFile('wsl.exe', ['-e', 'sh', '-c', wsl.script()], { encoding: 'utf8', timeout: 5000, windowsHide: true }, async (error, stdout) => {
                    const terminals = {};
                    const parsed = Object.entries(wsl.parse(error ? '' : stdout));
                    const profiles = parsed.length ? await accounts.list().catch(() => null) : null;
                    for (const [id, { cwd, argv, env, distribution }] of parsed) {
                        const program = argv.length ? agent.programName(argv) : null;
                        // WSL 계정 폴더는 그 배포판 사용자의 ~/.paddock/agent-profiles 아래에 있다.
                        // 홈 경로가 같은 다른 배포판의 계정을 붙이지 않도록 셸의 배포판 이름도 비교한다(모르면 비교하지 않는다).
                        const found = profiles && env.HOME ? attributeAccount({
                            provider: program, runtime: 'wsl', wslDistribution: distribution, env, profiles,
                            profileDirectory: profile => path.posix.join(env.HOME, '.paddock', 'agent-profiles', profile.id, profile.provider),
                        }) : null;
                        const linked = found?.kind === 'default' && distribution ? await this.linkWslDefault(program, env, distribution) : null;
                        terminals[id] = { cwd, program, account: publicAccount(linked ? { ...found, profile: linked } : found) };
                    }
                    response.json(terminals);
                });
            }
        });
        app.get('/paddock/wsl-ready', (request, response) => {
            readWslInfo().then(({ ready, home, distributions }) => response.json({ ready, home, distributions }));
        });
        app.get('/paddock/usage', async (request, response) => {
            try {
                const provider = request.query.provider;
                if (provider !== undefined && provider !== 'claude' && provider !== 'codex') throw usageError('provider must be claude or codex.', 400);
                if (Object.hasOwn(request.query, 'accountId')) {
                    response.json(await this.accountUsage.read(request.query.accountId));
                } else {
                    const result = { claude: { state: 'unset', windows: [], updatedAt: null }, codex: { windows: [], updatedAt: null } };
                    const wslInformation = await readWslInfo();
                    const wslHome = wslInformation.home;
                    if (provider !== 'codex') {
                        const directory = usageDirectory();
                        const state = readJson(path.join(directory, 'state.json'), {}).claude || 'unset';
                        // Windows 앱이면 WSL의 기본 Claude 수집 기록도 함께 보고 더 최근 것을 쓴다.
                        // 기록마다 그 환경의 로그인 정보 파일을 짝지어, 기록이 어느 계정의 것인지 확인한다(아래 linkDefault).
                        const claudeSources = [
                            { usageFile: path.join(directory, 'claude.json'), loginFile: path.join(process.env.CLAUDE_CONFIG_DIR || os.homedir(), '.claude.json'), where: { runtime: 'native' } },
                            ...(wslHome ? [{ usageFile: path.win32.join(wslHome, '.paddock', 'config', 'usage', 'claude.json'), loginFile: path.win32.join(wslHome, '.claude.json'), where: { runtime: 'wsl', wslDistribution: wslDistributionOf(wslHome) } }] : []),
                        ];
                        const newest = claudeSources.map(source => ({ ...source, record: readJson(source.usageFile, null) })).filter(source => source.record)
                            .sort((left, right) => (right.record.updated_at ?? 0) - (left.record.updated_at ?? 0))[0] ?? null;
                        let claudeFile = newest?.record ?? null;
                        let account = null;
                        // 기본 Claude를 등록 계정으로 보이는 것은 로그인한 기본 환경이 모두 그 계정 하나와 맞을 때뿐이다(Codex와 같은 규칙).
                        // 가장 최근 기록의 환경만 보면, 다른 환경의 `claude`가 다른 계정을 쓰는데도 그 계정에 'Default'가 붙는다.
                        const linked = await this.linkDefault('claude', claudeSources);
                        const match = linked ? await this.accountUsage.claudeRecord(linked.id) : null;
                        if (match) {
                            // 같은 계정의 한도는 하나이므로 등록 계정 터미널에서 더 최근에 받은 값이 있으면 그것을 쓴다.
                            account = { id: match.profile.id, label: match.profile.label };
                            if (match.record && (match.record.updated_at ?? 0) > (claudeFile?.updated_at ?? -1)) claudeFile = match.record;
                        }
                        result.claude = { state, windows: claudeFile ? usage.claudeWindows(claudeFile) : [], updatedAt: claudeFile?.updated_at ?? null, ...(account ? { account } : {}) };
                    }
                    if (provider !== 'claude') {
                        const nativeCodex = process.env.CODEX_HOME || path.join(os.homedir(), '.codex');
                        const codexScopes = [{ profile: { runtime: 'native' }, configDir: nativeCodex, sessionsDirectory: path.join(nativeCodex, 'sessions') }];
                        const codexLogins = [{ loginFile: path.join(nativeCodex, 'auth.json'), where: { runtime: 'native' } }];
                        if (wslHome && wslInformation.linuxHome) {
                            codexScopes.push({ profile: { runtime: 'wsl' }, configDir: path.posix.join(wslInformation.linuxHome, '.codex'), sessionsDirectory: path.win32.join(wslHome, '.codex', 'sessions') });
                            codexLogins.push({ loginFile: path.win32.join(wslHome, '.codex', 'auth.json'), where: { runtime: 'wsl', wslDistribution: wslDistributionOf(wslHome) } });
                        }
                        const [codex, account] = await Promise.all([this.liveCodexUsage.readAvailable(codexScopes), this.linkDefault('codex', codexLogins)]);
                        result.codex = { ...codex, ...(account ? { account: { id: account.id, label: account.label } } : {}) };
                    }
                    response.json(result);
                }
            } catch (error) {
                response.status(error.statusCode || 500).json({ error: error.message || 'Usage could not be read.' });
            }
        });
        app.post('/paddock/usage/claude', (request, response) => {
            const { enabled } = request.query;
            if (enabled !== 'true' && enabled !== 'false') {
                // 값이 빠진 요청을 "끄기"로 오해해 사용자 설정을 바꾸지 않는다.
                response.status(400).json({ error: 'enabled must be true or false' });
            } else {
                const operation = Object.hasOwn(request.query, 'accountId')
                    ? this.accountUsage.setClaude(request.query.accountId, enabled === 'true', request.query.automatic === 'true')
                    : this.setClaude(enabled === 'true', request.query.automatic === 'true');
                operation
                    .then(result => response.json(result))
                    .catch(error => response.status(error.statusCode || 500).json({ error: error instanceof Error ? error.message : String(error) }));
            }
        });
    }

    /**
     * 기본 CLI 사용량 행이 가리키는 등록 계정. 로그인한 기본 환경(이 컴퓨터·WSL)이 모두 같은 등록 계정 하나와 맞을 때만 그 계정이다.
     * 기본 사용량은 두 환경 중 한쪽(Codex는 값이 있는 쪽, Claude는 더 최근 기록)을 보이므로,
     * 한 환경이라도 다른 로그인이면 어느 계정의 값인지, 그리고 그냥 `codex`·`claude`가 어느 계정을 쓰는지 단정할 수 없어 null이다.
     * 로그인하지 않은(로그인 정보 파일이 없거나 로그인 기록이 없는) 환경은 대조에서 빠진다.
     * 등록 계정과 대조되지 않은 환경도 로그인 식별자가 대조된 환경과 같으면 같은 계정으로 센다 — 등록 계정이 WSL에만 있고
     * 이 컴퓨터에도 같은 로그인이 남아 있으면, 이 컴퓨터 쪽은 대조할 등록 계정이 없을 뿐 다른 계정이 아니기 때문이다.
     *
     * Parameters
     * ----------
     * provider : 'claude' 또는 'codex'.
     * sources : 기본 환경마다 `{ loginFile, where: { runtime, wslDistribution? } }`. loginFile은 이 앱이 읽을 수 있는 경로다.
     *
     * Examples
     * --------
     * | 이 컴퓨터    | WSL          | 결과 |
     * | ------------ | ------------ | ---- |
     * | A로 로그인   | 로그인 없음  | A    |
     * | A로 로그인   | A로 로그인   | A    |
     * | A의 로그인(이 컴퓨터에 등록 계정 없음) | A로 로그인 | A |
     * | 미등록 B     | A로 로그인   | null |
     * | 로그인 없음  | 로그인 없음  | null |
     */
    async linkDefault(
        provider,
        sources,
    ) {
        const logins = sources.map(source => ({ ...source, login: readLogin(provider, source.loginFile) })).filter(source => source.login);
        const matches = await Promise.all(logins.map(source => this.accountUsage.matchLogin({ provider, login: source.login, ...source.where }).catch(() => null)));
        const matched = matches.find(Boolean) || null;
        // 대조된 환경의 로그인 식별자. 대조되지 않은 환경은 이 식별자와 같을 때만 같은 계정이다.
        const matchedLogins = new Set(logins.filter((source, index) => matches[index]).map(source => source.login));
        const consistent = Boolean(matched) && matches.every((profile, index) => (profile ? profile.id === matched.id : matchedLogins.has(logins[index].login)));
        return consistent ? matched : null;
    }

    /**
     * WSL 터미널의 기본 CLI가 같은 배포판의 등록 계정 하나와 같은 로그인인지 확인해 그 계정을 돌려준다. 확인할 수 없으면 null이다.
     * 기본 폴더는 그 셸의 환경(HOME·CODEX_HOME·CLAUDE_CONFIG_DIR)으로 정하고, 등록 계정 폴더와 같은 방식(\\wsl.localhost 경로)으로 읽는다.
     */
    async linkWslDefault(
        provider,
        env,
        distribution,
    ) {
        let profile = null;
        try {
            const { toLocal } = wslFileMapping(await readWslInfo(distribution), distribution);
            const file = defaultLoginFile(provider, env, path.posix.join);
            if (file) profile = await this.accountUsage.matchLogin({ provider, runtime: 'wsl', wslDistribution: distribution, login: readLogin(provider, toLocal(file)) });
        } catch {
            profile = null;
        }
        return profile;
    }

    /**
     * 사용량 표시를 켜거나 끈다. 이 컴퓨터의 Claude Code 설정에 Paddock 상태 줄을 넣거나 원래대로 되돌린다.
     * Windows 앱이면 WSL 안의 Claude Code 설정에도 같은 일을 한다 — WSL에서 실행한 Claude는 WSL 쪽 설정만 읽는다.
     * 돌려주는 state는 이 컴퓨터 쪽 결과다('on'·'off', 자동 켜기를 건너뛰었으면 'unset').
     */
    async setClaude(
        enabled,
        isAutomatic = false,
    ) {
        const directory = usageDirectory();
        // 앱을 옮기거나 업데이트해도 Claude Code가 부르는 경로가 바뀌지 않게 스크립트를 사용량 폴더로 복사해 둔다.
        // 백엔드는 하나의 번들로 묶이므로 원본은 앱 폴더의 paddock-shell에서 찾는다(설치 파일에도 포함된다).
        const appPath = process.env.THEIA_APP_PROJECT_PATH || process.cwd();
        const source = path.join(appPath, 'paddock-shell', 'claude-statusline.cjs');
        const script = path.join(directory, 'claude-statusline.cjs');
        fs.copyFileSync(source, script);
        const previousFile = path.join(directory, 'previous-statusline.json');
        const command = statusLineCommand(script, path.join(directory, 'claude.json'), previousFile);
        if (enabled && !command) {
            throw new Error('Node.js is needed to show Claude usage. Install Node.js and try again.');
        }
        // 설정 파일이 링크면 링크를 끊지 않도록 실제 파일에 쓴다.
        const state = applyStatusLine({ settingsPath: settingsTarget(claudeSettingsPath()), previousFile, command, enabled, isAutomatic, toLocal: file => file });
        fs.writeFileSync(path.join(directory, 'state.json'), JSON.stringify({ claude: state }));
        const wslInfo = await readWslInfo();
        if (wslInfo.home && wslInfo.linuxHome && wslInfo.node) {
            // WSL 쪽 파일은 Windows 경로(\\wsl.localhost\…)로 쓰고, 상태 줄 명령에는 WSL 안에서 읽을 Linux 경로를 넣는다.
            // 사용량 기록은 WSL용 Paddock과 같은 자리(~/.paddock/config/usage)에 남아 어느 쪽 앱에서도 읽힌다.
            const root = wslInfo.home.slice(0, wslInfo.home.length - wslInfo.linuxHome.replace(/\//g, '\\').length);
            const toLocal = file => (file.startsWith('/') ? `${root}${file.replace(/\//g, '\\')}` : file);
            const linuxDirectory = `${wslInfo.linuxHome}/.paddock/config/usage`;
            fs.mkdirSync(toLocal(linuxDirectory), { recursive: true });
            fs.copyFileSync(source, toLocal(`${linuxDirectory}/claude-statusline.cjs`));
            const quote = value => usage.quoteStatusLineArgument(value);
            const linuxCommand = [wslInfo.node, `${linuxDirectory}/claude-statusline.cjs`, `${linuxDirectory}/claude.json`, `${linuxDirectory}/previous-statusline.json`].map(quote).join(' ');
            // WSL 설정 파일이 링크(예: dotfiles 저장소)면 WSL 안에서 링크를 따라가 실제 파일에 쓴다. Windows 쪽에서는 링크를 따라가지 못한다.
            const wslSettings = await resolveWslLink(`${wslInfo.linuxHome}/.claude/settings.json`);
            applyStatusLine({ settingsPath: toLocal(wslSettings), previousFile: toLocal(`${linuxDirectory}/previous-statusline.json`), command: linuxCommand, enabled, isAutomatic, toLocal });
        }
        return { state };
    }

}
decorate(injectable(), PaddockStatusRoutes);

exports.default = new ContainerModule((bind) => {
    bind(PaddockStatusRoutes).toSelf().inSingletonScope();
    bind(BackendApplicationContribution).toService(PaddockStatusRoutes);
    // Git 보기를 그리는 내장 git 확장은 플러그인 호스트 프로세스에서 git을 실행하므로, 그 프로세스 환경에만 WSL 저장소 신뢰를 넣는다.
    bind(PluginHostEnvironmentVariable).toConstantValue({ process: env => trustWslRepositories(env, process.platform) });
    // Expose account actions only; filesystem helpers are not remotely callable.
    bind(ConnectionHandler).toConstantValue(new RpcConnectionHandler(AccountProfile.SERVICE_PATH, () => ({
        list: () => accounts.list(),
        create: input => accounts.create(input),
        rename: (id, label) => accounts.rename(id, label),
        remove: id => accounts.remove(id),
        prepare: id => accounts.prepare(id),
        inspectStorageAccess: request => accounts.inspectStorageAccess(request),
        allowStorageAccess: request => accounts.allowStorageAccess(request),
        openStorageSettings: request => accounts.openStorageSettings(request),
        observeSession: request => accountSessions.observeSession(request),
        observeSessions: requests => accountSessions.observeSessions(requests),
        prepareResume: (id, request) => accountSessions.prepareResume(id, request),
        stopSession: request => accountSessions.stopSession(request),
    })));
});
