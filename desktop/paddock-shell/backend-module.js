Object.defineProperty(exports, "__esModule", { value: true });
// 상태 줄 게이지(메모리·AI 사용량)에 쓰는 값을 백엔드에서 읽어 준다. 원격 창이면 그 호스트의 값이다.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { ContainerModule, injectable, decorate } = require('@theia/core/shared/inversify');
const { BackendApplicationContribution } = require('@theia/core/lib/node/backend-application');
const { memoryPercent } = require('./work-model');
const usage = require('./usage-model');
const agent = require('./agent-model');

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

/** 가장 최근 Codex 세션 기록 파일. 날짜 폴더(YYYY/MM/DD)를 최신부터 훑어 처음 나온 이틀 치에서 고른다. */
function latestCodexSession() {
    const root = path.join(process.env.CODEX_HOME || path.join(os.homedir(), '.codex'), 'sessions');
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
 * Linux는 /proc, macOS는 ps로 읽는다. 읽을 수 없거나 Windows면 null.
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
 * `GET /paddock/foreground?pids=1,2` → `{ "1": "claude", "2": "bash" }` 셸 pid별 실행 중 프로그램 이름(모르면 null).
 * `POST /paddock/usage/claude?enabled=true|false` → Claude Code 설정에 Paddock 상태 줄을 넣거나 원래대로 되돌린다.
 * Claude의 `state`는 'unset'(아직 정한 적 없음)·'on'·'off'다.
 */
class PaddockStatusRoutes {
    configure(
        app,
    ) {
        app.get('/paddock/memory', (request, response) => {
            const total = os.totalmem();
            const free = os.freemem();
            response.json({ percent: memoryPercent(total, free), total, free });
        });
        app.get('/paddock/foreground', (request, response) => {
            const names = {};
            for (const pid of String(request.query.pids || '').split(',').filter(value => /^\d+$/.test(value))) {
                const argv = foregroundArgv(Number(pid));
                names[pid] = argv ? agent.programName(argv) : null;
            }
            response.json(names);
        });
        app.get('/paddock/usage', (request, response) => {
            const directory = usageDirectory();
            const state = readJson(path.join(directory, 'state.json'), {}).claude || 'unset';
            const claudeFile = readJson(path.join(directory, 'claude.json'), null);
            let codex = { windows: [], updatedAt: null };
            try {
                const session = latestCodexSession();
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
                try {
                    response.json(this.setClaude(enabled === 'true'));
                } catch (error) {
                    response.status(500).json({ error: error instanceof Error ? error.message : String(error) });
                }
            }
        });
    }

    setClaude(
        enabled,
    ) {
        const directory = usageDirectory();
        const settingsPath = claudeSettingsPath();
        const previousFile = path.join(directory, 'previous-statusline.json');
        const script = path.join(directory, 'claude-statusline.cjs');
        // 앱을 옮기거나 업데이트해도 Claude Code가 부르는 경로가 바뀌지 않게 스크립트를 사용량 폴더로 복사해 둔다.
        // 백엔드는 하나의 번들로 묶이므로 원본은 앱 폴더의 paddock-shell에서 찾는다(설치 파일에도 포함된다).
        const appPath = process.env.THEIA_APP_PROJECT_PATH || process.cwd();
        fs.copyFileSync(path.join(appPath, 'paddock-shell', 'claude-statusline.cjs'), script);
        const command = statusLineCommand(script, path.join(directory, 'claude.json'), previousFile);
        let settings = {};
        if (fs.existsSync(settingsPath)) {
            settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
            // 처음 바꿀 때 한 번만 원본을 남긴다.
            if (!fs.existsSync(`${settingsPath}.paddock-backup`)) fs.copyFileSync(settingsPath, `${settingsPath}.paddock-backup`);
        }
        if (enabled && !command) {
            throw new Error('Node.js is needed to show Claude usage. Install Node.js and try again.');
        }
        let next = settings;
        if (enabled) {
            const installed = usage.installStatusLine(settings, command);
            if (installed.previous) fs.writeFileSync(previousFile, JSON.stringify(installed.previous));
            next = installed.settings;
        } else if (settings.statusLine?.command === command) {
            next = usage.restoreStatusLine(settings, readJson(previousFile, null));
            fs.rmSync(previousFile, { force: true });
        }
        fs.mkdirSync(path.dirname(settingsPath), { recursive: true });
        fs.writeFileSync(settingsPath, `${JSON.stringify(next, null, 2)}\n`);
        fs.writeFileSync(path.join(directory, 'state.json'), JSON.stringify({ claude: enabled ? 'on' : 'off' }));
        return { state: enabled ? 'on' : 'off' };
    }
}
decorate(injectable(), PaddockStatusRoutes);

exports.default = new ContainerModule((bind) => {
    bind(PaddockStatusRoutes).toSelf().inSingletonScope();
    bind(BackendApplicationContribution).toService(PaddockStatusRoutes);
});
