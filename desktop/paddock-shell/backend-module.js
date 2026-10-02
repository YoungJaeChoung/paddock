Object.defineProperty(exports, "__esModule", { value: true });
// 상태 줄 게이지(메모리·AI 사용량)에 쓰는 값을 백엔드에서 읽어 준다. 원격 창이면 그 호스트의 값이다.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFile, spawn, spawnSync } = require('node:child_process');
const express = require('express');
const { ContainerModule, injectable, decorate } = require('@theia/core/shared/inversify');
const { BackendApplicationContribution } = require('@theia/core/lib/node/backend-application');
const { memoryPercent } = require('./work-model');
const usage = require('./usage-model');
const agent = require('./agent-model');
const { WorkPresenceRegistry } = require('./work-presence');
const wsl = require('./wsl-terminals');
const windowsProcesses = require('./windows-processes');

// Codex 세션 기록 끝부분만 읽는다. 마지막 한도 기록은 파일 끝 가까이에 있다.
const N_TAIL_BYTES = 512 * 1024;

function usageDirectory() {
    const directory = path.join(process.env.THEIA_CONFIG_DIR || path.join(os.homedir(), '.paddock', 'config'), 'usage');
    fs.mkdirSync(directory, { recursive: true });
    return directory;
}

function claudeSettingsPath() {
    return path.join(process.env.CLAUDE_CONFIG_DIR || path.join(os.homedir(), '.claude'), 'settings.json');
}

function readJson(
    file,
    fallback,
) {
    let value = fallback;
    try {
        value = JSON.parse(fs.readFileSync(file, 'utf8'));
    } catch {
        value = fallback;
    }
    return value;
}

function readTail(
    file,
) {
    const handle = fs.openSync(file, 'r');
    const size = fs.fstatSync(handle).size;
    const start = Math.max(0, size - N_TAIL_BYTES);
    const buffer = Buffer.alloc(size - start);
    fs.readSync(handle, buffer, 0, buffer.length, start);
    fs.closeSync(handle);
    return buffer.toString('utf8');
}

/** `root`(Codex 세션 폴더) 안의 가장 최근 세션 기록 파일. 날짜 폴더(YYYY/MM/DD)를 최신부터 훑어 처음 나온 이틀 치에서 고른다. 없으면 null. */
function latestCodexSession(
    root,
) {
    const newestFirst = (directory) => (fs.existsSync(directory) ? fs.readdirSync(directory).sort().reverse().map(name => path.join(directory, name)) : []);
    const files = [];
    for (const year of newestFirst(root)) {
        for (const month of newestFirst(year)) {
            for (const day of newestFirst(month)) {
                if (files.length < 50 && fs.statSync(day).isDirectory()) {
                    files.push(...newestFirst(day).filter(file => file.endsWith('.jsonl')));
                }
            }
        }
    }
    let latest = null;
    for (const file of files) {
        const modified = fs.statSync(file).mtimeMs;
        if (!latest || modified > latest.modified) latest = { file, modified };
    }
    return latest;
}

/**
 * 셸 프로세스의 지금 앞쪽 프로그램(터미널에서 실행 중인 명령)의 명령줄. 셸만 떠 있으면 셸의 명령줄이다.
 * Linux는 /proc, macOS는 ps로 읽는다. 읽을 수 없으면 null. Windows는 WindowsProcessList와 windows-processes.js가 맡는다.
 */
function foregroundArgv(
    shellPid,
) {
    let argv = null;
    try {
        if (process.platform === 'linux') {
            // stat의 8번째 칸이 터미널 앞쪽 프로세스 그룹. 프로그램 이름에 공백·괄호가 있을 수 있어 마지막 ')' 뒤부터 센다.
            const stat = fs.readFileSync(`/proc/${shellPid}/stat`, 'utf8');
            const foreground = Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[5]);
            const pid = foreground > 0 ? foreground : shellPid;
            argv = fs.readFileSync(`/proc/${pid}/cmdline`, 'utf8').split('\0').filter(Boolean);
        } else if (process.platform === 'darwin') {
            const group = spawnSync('ps', ['-o', 'tpgid=', '-p', String(shellPid)], { encoding: 'utf8' }).stdout.trim();
            const pid = Number(group) > 0 ? group : String(shellPid);
            const args = spawnSync('ps', ['-o', 'args=', '-p', pid], { encoding: 'utf8' }).stdout.trim();
            argv = args ? args.split(/\s+/) : null;
        }
    } catch {
        argv = null;
    }
    return argv;
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
        '[pscustomobject]@{ pid = $_.ProcessId; parentPid = $_.ParentProcessId; commandLine = $_.CommandLine; createdAt = $_.CreationDate.Ticks } });',
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

let wslInfoPromise = null;

/**
 * Windows에 설치된 기본 WSL 배포판의 정보: `{ ready, home, linuxHome, node }`.
 * home은 홈 폴더의 Windows 경로(\\wsl.localhost\…), linuxHome은 Linux 경로, node는 WSL 안 node 실행 파일(없으면 '')이다.
 * Windows가 아니거나 배포판이 없으면 ready가 false다. 성공한 결과만 기억하고, 실패하면 다음 호출에서 다시 읽는다.
 */
function readWslInfo() {
    if (!wslInfoPromise) {
        const empty = { ready: false, home: '', linuxHome: '', node: '' };
        wslInfoPromise = process.platform !== 'win32' ? Promise.resolve(empty) : new Promise((resolve) => {
            // wsl.exe -l은 UTF-16으로 배포판 이름을 한 줄씩 출력한다. 배포판이 없으면 오류로 끝난다.
            execFile('wsl.exe', ['-l', '-q'], { encoding: 'buffer', timeout: 5000, windowsHide: true }, (error, stdout) => {
                const names = error ? [] : stdout.toString('utf16le').split(/\r?\n/).map(name => name.replace(/\0/g, '').trim()).filter(Boolean);
                if (!names.length) {
                    resolve(empty);
                } else {
                    // nvm처럼 대화형 셸 설정에서 PATH를 잡는 node도 찾도록 bash -i로 읽는다. 설정 파일이 찍는 글은 앞쪽 줄이라 끝 세 줄만 쓴다.
                    // WSL을 켜는 데 몇 초 걸릴 수 있다.
                    const probe = 'printf "\\n%s\\n%s\\n%s" "$(wslpath -w "$HOME")" "$HOME" "$(command -v node)"';
                    execFile('wsl.exe', ['-e', 'bash', '-ic', probe], { encoding: 'utf8', timeout: 20000, windowsHide: true }, (probeError, output) => {
                        const [home = '', linuxHome = '', node = ''] = (probeError ? '' : output).replace(/\r/g, '').split('\n').slice(-3);
                        resolve({ ready: true, home: home.trim(), linuxHome: linuxHome.trim(), node: node.trim() });
                    });
                }
            });
        });
        wslInfoPromise.then((info) => {
            if (process.platform === 'win32' && (!info.ready || !info.home)) wslInfoPromise = null;
        });
    }
    return wslInfoPromise;
}

function statusLineCommand(
    script,
    usageFile,
    previousFile,
) {
    const finder = process.platform === 'win32' ? 'where' : 'which';
    const found = spawnSync(finder, ['node'], { encoding: 'utf8' }).stdout?.split(/\r?\n/)[0]?.trim();
    const quote = (value) => `"${value}"`;
    let runner = null;
    if (found) runner = quote(found);
    else if (process.platform !== 'win32') runner = `ELECTRON_RUN_AS_NODE=1 ${quote(process.execPath)}`;
    return runner ? `${runner} ${quote(script)} ${quote(usageFile)} ${quote(previousFile)}` : null;
}

/**
 * `GET /paddock/memory` → `{ percent, total, free }`.
 * `GET /paddock/usage` → `{ claude: { state, windows, updatedAt }, codex: { windows, updatedAt } }`.
 * `GET /paddock/foreground?pids=1,2` → `{ "1": "claude", "2": "bash" }` 셸 pid별 실행 중 프로그램 이름(모르면 null). Windows는 프로세스 목록으로 찾는다.
 * `GET /paddock/wsl-terminals` → `{ [터미널 id]: { cwd, program } }` Windows 앱의 WSL 터미널별 현재 폴더(Windows 경로)·실행 중 프로그램.
 *   Windows가 아니거나 WSL을 읽지 못하면 `{}`다.
 * `GET /paddock/wsl-ready` → `{ ready, home }` 이 Windows에 WSL 배포판이 하나라도 설치돼 있는지와 기본 배포판의 홈 폴더(Windows 경로).
 *   wsl.exe만 있고 배포판이 없으면 `{ ready: false, home: '' }`다.
 * `POST /paddock/usage/claude?enabled=true|false[&automatic=true]` → Claude Code 설정에 Paddock 상태 줄을 넣거나 원래대로 되돌린다.
 *   automatic이면(처음 실행의 자동 켜기) 다른 Paddock 설정 폴더의 상태 줄이 살아 있을 때 바꾸지 않고 `{ state: 'unset' }`을 돌려준다.
 * Claude의 `state`는 'unset'(아직 정한 적 없음)·'on'·'off'다.
 */
class PaddockStatusRoutes {
    constructor() {
        this.workPresence = new WorkPresenceRegistry();
        this.windowsProcesses = new WindowsProcessList();
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
        app.get('/paddock/memory', (request, response) => {
            const total = os.totalmem();
            const free = os.freemem();
            response.json({ percent: memoryPercent(total, free), total, free });
        });
        app.get('/paddock/foreground', async (request, response) => {
            const names = {};
            const pids = String(request.query.pids || '').split(',').filter(value => /^\d+$/.test(value));
            // Windows에는 터미널의 앞쪽 프로세스가 없어 프로세스 목록에서 셸의 자식을 찾는다. 목록을 못 읽으면 모두 null이다.
            const processes = process.platform === 'win32' && pids.length ? await this.windowsProcesses.read().catch(() => []) : [];
            for (const pid of pids) {
                const argv = process.platform === 'win32' ? windowsProcesses.foregroundArgv(processes, Number(pid)) : foregroundArgv(Number(pid));
                names[pid] = argv ? agent.programName(argv) : null;
            }
            response.json(names);
        });
        app.get('/paddock/wsl-terminals', (request, response) => {
            if (process.platform !== 'win32') {
                response.json({});
            } else {
                // WSL이 꺼져 있으면 켜는 데 몇 초 걸린다. 늦으면 이번 주기는 비우고 다음 주기에 다시 읽는다.
                execFile('wsl.exe', ['-e', 'sh', '-c', wsl.script()], { encoding: 'utf8', timeout: 5000, windowsHide: true }, (error, stdout) => {
                    const terminals = {};
                    for (const [id, { cwd, argv }] of Object.entries(wsl.parse(error ? '' : stdout))) {
                        terminals[id] = { cwd, program: argv.length ? agent.programName(argv) : null };
                    }
                    response.json(terminals);
                });
            }
        });
        app.get('/paddock/wsl-ready', (request, response) => {
            readWslInfo().then(({ ready, home }) => response.json({ ready, home }));
        });
        app.get('/paddock/usage', async (request, response) => {
            const directory = usageDirectory();
            const state = readJson(path.join(directory, 'state.json'), {}).claude || 'unset';
            // Windows 앱이면 WSL에서 실행한 Claude·Codex의 기록도 함께 보고 더 최근 것을 쓴다. CLI를 어느 쪽에서 실행했는지 모른다.
            const wslHome = (await readWslInfo()).home;
            const claudeFiles = [path.join(directory, 'claude.json'), ...(wslHome ? [path.win32.join(wslHome, '.paddock', 'config', 'usage', 'claude.json')] : [])];
            const claudeFile = claudeFiles.map(file => readJson(file, null)).filter(Boolean)
                .sort((left, right) => (right.updated_at ?? 0) - (left.updated_at ?? 0))[0] ?? null;
            let codex = { windows: [], updatedAt: null };
            try {
                const roots = [path.join(process.env.CODEX_HOME || path.join(os.homedir(), '.codex'), 'sessions'), ...(wslHome ? [path.win32.join(wslHome, '.codex', 'sessions')] : [])];
                const session = roots.map(root => latestCodexSession(root)).filter(Boolean).sort((left, right) => right.modified - left.modified)[0];
                if (session) codex = { windows: usage.codexWindows(readTail(session.file)), updatedAt: Math.floor(session.modified / 1000) };
            } catch {
                // 기록을 못 읽으면 게이지를 비워 둔다.
            }
            response.json({
                claude: { state, windows: claudeFile ? usage.claudeWindows(claudeFile) : [], updatedAt: claudeFile?.updated_at ?? null },
                codex,
            });
        });
        app.post('/paddock/usage/claude', (request, response) => {
            const { enabled } = request.query;
            if (enabled !== 'true' && enabled !== 'false') {
                // 값이 빠진 요청을 "끄기"로 오해해 사용자 설정을 바꾸지 않는다.
                response.status(400).json({ error: 'enabled must be true or false' });
            } else {
                this.setClaude(enabled === 'true', request.query.automatic === 'true')
                    .then(result => response.json(result))
                    .catch(error => response.status(500).json({ error: error instanceof Error ? error.message : String(error) }));
            }
        });
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
        const state = this.applyStatusLine({ settingsPath: claudeSettingsPath(), previousFile, command, enabled, isAutomatic, toLocal: file => file });
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
            const quote = value => `"${value}"`;
            const linuxCommand = [wslInfo.node, `${linuxDirectory}/claude-statusline.cjs`, `${linuxDirectory}/claude.json`, `${linuxDirectory}/previous-statusline.json`].map(quote).join(' ');
            this.applyStatusLine({ settingsPath: toLocal(`${wslInfo.linuxHome}/.claude/settings.json`), previousFile: toLocal(`${linuxDirectory}/previous-statusline.json`), command: linuxCommand, enabled, isAutomatic, toLocal });
        }
        return { state };
    }

    /**
     * Claude Code 설정 파일 하나에 Paddock 상태 줄을 넣거나 되돌리고 결과 상태를 돌려준다.
     * Claude Code 설정은 컴퓨터(또는 WSL)마다 하나뿐이라, 다른 설정 폴더의 Paddock(설치본·시험용 실행)이 이미 쓰는 중이면
     * 자동 켜기는 빼앗지 않고 'unset'을 돌려준다. 그 스크립트가 지워졌으면(끝난 시험용 실행) 주인이 없는 것으로 보고 넘겨받는다.
     * `toLocal`은 명령 속 경로를 이 프로세스가 읽을 수 있는 경로로 바꾼다(WSL 쪽이면 Linux 경로 → \\wsl.localhost\…).
     */
    applyStatusLine(
        { settingsPath, previousFile, command, enabled, isAutomatic, toLocal },
    ) {
        let settings = {};
        if (fs.existsSync(settingsPath)) {
            settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
            // 처음 바꿀 때 한 번만 원본을 남긴다.
            if (!fs.existsSync(`${settingsPath}.paddock-backup`)) fs.copyFileSync(settingsPath, `${settingsPath}.paddock-backup`);
        }
        const owner = usage.paddockStatusLineFiles(settings.statusLine?.command);
        const isTakenByOther = Boolean(owner) && settings.statusLine.command !== command && fs.existsSync(toLocal(owner.script));
        let state = enabled ? 'on' : 'off';
        if (enabled && isAutomatic && isTakenByOther) {
            state = 'unset';
        } else {
            let next = settings;
            if (enabled) {
                const installed = usage.installStatusLine(settings, command);
                // 다른 Paddock 명령을 바꾸면 그 명령이 이어 부르던 원래 상태 줄을 넘겨받는다. Paddock 명령끼리는 잇지 않는다.
                const inherited = installed.replaced ? readJson(toLocal(installed.replaced.previousFile), null) : null;
                const previous = installed.previous || (inherited && !usage.paddockStatusLineFiles(inherited.command) ? inherited : null);
                if (previous) fs.writeFileSync(previousFile, JSON.stringify(previous));
                next = installed.settings;
            } else if (settings.statusLine?.command === command) {
                next = usage.restoreStatusLine(settings, readJson(previousFile, null));
                fs.rmSync(previousFile, { force: true });
            }
            if (next !== settings) {
                fs.mkdirSync(path.dirname(settingsPath), { recursive: true });
                fs.writeFileSync(settingsPath, `${JSON.stringify(next, null, 2)}\n`);
            }
        }
        return state;
    }
}
decorate(injectable(), PaddockStatusRoutes);

exports.default = new ContainerModule((bind) => {
    bind(PaddockStatusRoutes).toSelf().inSingletonScope();
    bind(BackendApplicationContribution).toService(PaddockStatusRoutes);
});
