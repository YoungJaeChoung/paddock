const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const backend = require('../backend-module');
const { AccountProfiles } = require('../account-profiles');

// 백엔드의 계정 목록은 모듈을 불러올 때 정한 설정 폴더를 쓴다. 계정 폴더 경로를 같은 규칙으로 계산한다.
const BACKEND_CONFIG = new AccountProfiles().configDirectory;

function fixture(
    context,
    seed = () => {},
) {
    let Contribution;
    backend.default.registry(token => {
        if (token?.name === 'PaddockStatusRoutes') Contribution = token;
        return { toSelf: () => ({ inSingletonScope() {} }), toService() {}, toConstantValue() {}, toDynamicValue: () => ({ inSingletonScope() {} }) };
    });
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-usage-route-'));
    // 테스트를 Paddock 계정 터미널에서 돌리면 CLAUDE_CONFIG_DIR·CODEX_HOME이 실제 계정 폴더를 가리킨다.
    // 그대로 두면 실제 로그인 계정과 사용량 기록이 픽스처 값을 덮으므로, 홈과 함께 임시 폴더로 돌린다.
    const isolated = { THEIA_CONFIG_DIR: root, HOME: root, CLAUDE_CONFIG_DIR: undefined, CODEX_HOME: undefined };
    const previous = Object.fromEntries(Object.keys(isolated).map(name => [name, process.env[name]]));
    const assign = values => {
        for (const [name, value] of Object.entries(values)) {
            if (value === undefined) delete process.env[name];
            else process.env[name] = value;
        }
    };
    assign(isolated);
    seed(root);
    const contribution = new Contribution();
    const routes = new Map();
    contribution.configure({ get: (url, route) => routes.set(url, route), post: (url, route) => routes.set(`POST ${url}`, route), delete() {} });
    // 시작 때 비동기로 이어지는 작업(설정 파일 쓰기·예전 항목 걷어 내기)이 끝난 뒤에 환경을 되돌리고 임시 폴더를 지운다.
    context.after(async () => {
        await contribution.pendingDefaultWrites;
        assign(previous);
        fs.rmSync(root, { recursive: true, force: true });
    });
    const call = async query => {
        let value;
        let status = 200;
        await routes.get('/paddock/usage')({ query }, { json: data => { value = data; }, status: code => { status = code; return { json: data => { value = data; } }; } });
        return { value, status };
    };
    return { root, contribution, call, routes };
}

test('UR01 a Claude-only default lookup never starts Codex even if its CLI is installed', async context => {
    const { root, contribution, call } = fixture(context);
    fs.mkdirSync(path.join(root, 'usage'));
    fs.writeFileSync(path.join(root, 'usage/claude.json'), JSON.stringify({ rate_limits: { five_hour: { used_percentage: 15 } }, updated_at: 100 }));
    const read = context.mock.method(contribution.liveCodexUsage, 'readAvailable', async () => { throw new Error('Codex must remain idle'); });
    const response = await call({ provider: 'claude' });
    assert.equal(response.status, 200);
    assert.equal(response.value.claude.windows[0].used, 15);
    assert.equal(read.mock.callCount(), 0);
});

test('UR02 a Codex-only default lookup ignores an unreadable Claude summary and rejects invalid provider names', async context => {
    const { root, contribution, call } = fixture(context);
    fs.mkdirSync(path.join(root, 'usage'));
    fs.writeFileSync(path.join(root, 'usage/claude.json'), '{unfinished');
    context.mock.method(contribution.liveCodexUsage, 'readAvailable', async () => ({ windows: [{ label: 'week', used: 83 }], updatedAt: 100 }));
    const response = await call({ provider: 'codex' });
    assert.equal(response.status, 200);
    assert.equal(response.value.codex.windows[0].used, 83);
    assert.equal((await call({ provider: ['claude', 'codex'] })).status, 400);
    assert.equal((await call({ provider: 'unknown' })).status, 400);
});

test('UR03 the default Claude lookup names the matching saved account and uses its newer limits', async context => {
    const { root, contribution, call } = fixture(context);
    const previous = process.env.CLAUDE_CONFIG_DIR;
    process.env.CLAUDE_CONFIG_DIR = root;
    context.after(() => { if (previous === undefined) delete process.env.CLAUDE_CONFIG_DIR; else process.env.CLAUDE_CONFIG_DIR = previous; });
    fs.mkdirSync(path.join(root, 'usage'));
    fs.writeFileSync(path.join(root, 'usage/claude.json'), JSON.stringify({ rate_limits: { five_hour: { used_percentage: 15 } }, updated_at: 100 }));
    fs.writeFileSync(path.join(root, '.claude.json'), JSON.stringify({ oauthAccount: { accountUuid: 'main-login' } }));
    const match = context.mock.method(contribution.accountUsage, 'matchLogin', async ({ provider, login }) => (provider === 'claude' && login === 'main-login'
        ? { id: 'main-id', label: 'Main', provider: 'claude', runtime: 'native' } : null));
    context.mock.method(contribution.accountUsage, 'claudeRecord', async id => (id === 'main-id'
        ? { profile: { id: 'main-id', label: 'Main' }, record: { rate_limits: { five_hour: { used_percentage: 40 } }, updated_at: 200 } } : null));
    const linked = await call({ provider: 'claude' });
    assert.deepEqual(linked.value.claude.account, { id: 'main-id', label: 'Main' });
    assert.equal(linked.value.claude.windows[0].used, 40);
    fs.writeFileSync(path.join(root, '.claude.json'), JSON.stringify({ oauthAccount: { accountUuid: 'other-login' } }));
    const unlinked = await call({ provider: 'claude' });
    assert.equal(unlinked.value.claude.account, undefined);
    assert.equal(unlinked.value.claude.windows[0].used, 15);
    assert.equal(match.mock.callCount(), 2);
});

test('UR04 foreground reports the account from the running CLI environment, not the terminal that opened it', { skip: process.platform !== 'linux' }, async context => {
    const { root, routes } = fixture(context);
    const workCodex = { id: '11111111-1111-4111-8111-111111111111', provider: 'codex', label: 'Work Codex', runtime: 'native' };
    const workClaude = { id: '22222222-2222-4222-8222-222222222222', provider: 'claude', label: 'Work Claude', runtime: 'native' };
    context.mock.method(AccountProfiles.prototype, 'list', async () => [workCodex, workClaude]);
    const script = path.join(root, 'codex.js');
    fs.writeFileSync(script, 'setInterval(() => {}, 1000);');
    const start = env => {
        const child = spawn(process.execPath, [script], { env: { PATH: process.env.PATH, HOME: root, OPENAI_API_KEY: 'secret', ...env }, stdio: 'ignore' });
        context.after(() => child.kill('SIGTERM'));
        return child.pid;
    };
    const profilePid = start({ CODEX_HOME: path.join(BACKEND_CONFIG, 'agent-profiles', workCodex.id, 'codex') });
    const defaultPid = start({ CLAUDE_CONFIG_DIR: path.join(BACKEND_CONFIG, 'agent-profiles', workClaude.id, 'claude') });
    const customPid = start({ CODEX_HOME: path.join(root, 'elsewhere') });
    await new Promise(resolve => setTimeout(resolve, 200));
    let value;
    await routes.get('/paddock/foreground')({ query: { pids: `${profilePid},${defaultPid},${customPid}` } }, { json: data => { value = data; } });
    assert.deepEqual(value[profilePid], { program: 'codex', account: { kind: 'profile', profile: workCodex } });
    assert.deepEqual(value[defaultPid], { program: 'codex', account: { kind: 'default' } });
    assert.deepEqual(value[customPid], { program: 'codex', account: { kind: 'custom' } });
    assert.equal(JSON.stringify(value).includes('secret'), false);
    assert.equal(JSON.stringify(value).includes('agent-profiles'), false);
});

test('UR05 the default Codex lookup names the saved account only when the default sign-in matches exactly one', async context => {
    const { root, contribution, call } = fixture(context);
    const previous = process.env.CODEX_HOME;
    process.env.CODEX_HOME = path.join(root, 'codex-home');
    context.after(() => { if (previous === undefined) delete process.env.CODEX_HOME; else process.env.CODEX_HOME = previous; });
    fs.mkdirSync(process.env.CODEX_HOME);
    fs.writeFileSync(path.join(process.env.CODEX_HOME, 'auth.json'), JSON.stringify({ tokens: { account_id: 'acc-main', access_token: 'secret-token' } }));
    context.mock.method(contribution.liveCodexUsage, 'readAvailable', async () => ({ windows: [{ label: 'week', used: 83 }], updatedAt: 100 }));
    const logins = [];
    context.mock.method(contribution.accountUsage, 'matchLogin', async request => {
        logins.push(request);
        return request.login === 'acc-main' ? { id: 'main-id', label: 'Main', provider: 'codex', runtime: 'native' } : null;
    });
    const linked = await call({ provider: 'codex' });
    assert.deepEqual(linked.value.codex.account, { id: 'main-id', label: 'Main' });
    assert.equal(linked.value.codex.windows[0].used, 83);
    assert.equal(logins[0].runtime, 'native');
    assert.equal(JSON.stringify(linked.value).includes('secret-token'), false);
    fs.writeFileSync(path.join(process.env.CODEX_HOME, 'auth.json'), JSON.stringify({ tokens: { account_id: 'acc-other' } }));
    assert.equal((await call({ provider: 'codex' })).value.codex.account, undefined);
    fs.rmSync(path.join(process.env.CODEX_HOME, 'auth.json'));
    assert.equal((await call({ provider: 'codex' })).value.codex.account, undefined, 'a signed-out default CLI is never linked');
});

test('UR06 foreground names the saved account for a default CLI whose sign-in matches it', { skip: process.platform !== 'linux' }, async context => {
    const { root, routes, contribution } = fixture(context);
    const workCodex = { id: '11111111-1111-4111-8111-111111111111', provider: 'codex', label: 'Work Codex', runtime: 'native' };
    context.mock.method(AccountProfiles.prototype, 'list', async () => [workCodex]);
    fs.mkdirSync(path.join(root, '.codex'));
    fs.writeFileSync(path.join(root, '.codex', 'auth.json'), JSON.stringify({ tokens: { account_id: 'acc-work', refresh_token: 'secret-refresh' } }));
    const requests = [];
    context.mock.method(contribution.accountUsage, 'matchLogin', async request => {
        requests.push(request);
        return request.login === 'acc-work' ? workCodex : null;
    });
    const script = path.join(root, 'codex.js');
    fs.writeFileSync(script, 'setInterval(() => {}, 1000);');
    const child = spawn(process.execPath, [script], { env: { PATH: process.env.PATH, HOME: root }, stdio: 'ignore' });
    context.after(() => child.kill('SIGTERM'));
    await new Promise(resolve => setTimeout(resolve, 200));
    const read = async () => {
        let value;
        await routes.get('/paddock/foreground')({ query: { pids: String(child.pid) } }, { json: data => { value = data; } });
        return value[child.pid];
    };
    const linked = await read();
    assert.deepEqual(linked, { program: 'codex', account: { kind: 'default', profile: workCodex } });
    assert.deepEqual(requests[0], { provider: 'codex', runtime: 'native', login: 'acc-work' });
    assert.equal(JSON.stringify(linked).includes('secret'), false);
    fs.writeFileSync(path.join(root, '.codex', 'auth.json'), JSON.stringify({ tokens: { account_id: 'acc-personal' } }));
    assert.deepEqual(await read(), { program: 'codex', account: { kind: 'default' } }, 'a different sign-in stays Current CLI');
});

test('UR07 the default CLI is named as a saved account only when every signed-in default environment matches that one account', async context => {
    const { root, contribution } = fixture(context);
    const write = (name, value) => {
        const file = path.join(root, name);
        if (value === undefined) fs.rmSync(file, { force: true });
        else fs.writeFileSync(file, JSON.stringify(value));
        return file;
    };
    const A = { id: 'a-id', label: 'A', provider: 'claude' };
    context.mock.method(contribution.accountUsage, 'matchLogin', async ({ login }) => (login === 'login-a' ? A : null));
    const sources = () => [
        { loginFile: path.join(root, 'native.json'), where: { runtime: 'native' } },
        { loginFile: path.join(root, 'wsl.json'), where: { runtime: 'wsl', wslDistribution: 'Ubuntu' } },
    ];
    write('native.json', { oauthAccount: { accountUuid: 'login-a' } });
    assert.equal(await contribution.linkDefault('claude', sources()), A, 'a signed-out environment is left out');
    write('wsl.json', { oauthAccount: { accountUuid: 'login-a' } });
    assert.equal(await contribution.linkDefault('claude', sources()), A);
    write('native.json', { oauthAccount: { accountUuid: 'login-unregistered' } });
    assert.equal(await contribution.linkDefault('claude', sources()), null, 'plain claude in one environment uses another account');
    write('native.json', undefined);
    write('wsl.json', undefined);
    assert.equal(await contribution.linkDefault('claude', sources()), null);
});

test('UR08 an environment with the same sign-in but no saved account in its runtime does not block the link', async context => {
    const { root, contribution } = fixture(context);
    const write = (name, value) => fs.writeFileSync(path.join(root, name), JSON.stringify(value));
    const A = { id: 'a-id', label: 'A', provider: 'claude' };
    // 등록 계정은 WSL에만 있다. 이 컴퓨터의 같은 로그인은 대조할 계정이 없어 matchLogin이 null이다.
    context.mock.method(contribution.accountUsage, 'matchLogin', async ({ login, runtime }) => (runtime === 'wsl' && login === 'login-a' ? A : null));
    const sources = [
        { loginFile: path.join(root, 'native.json'), where: { runtime: 'native' } },
        { loginFile: path.join(root, 'wsl.json'), where: { runtime: 'wsl', wslDistribution: 'Ubuntu' } },
    ];
    write('native.json', { oauthAccount: { accountUuid: 'login-a' } });
    write('wsl.json', { oauthAccount: { accountUuid: 'login-a' } });
    assert.equal(await contribution.linkDefault('claude', sources), A);
    write('native.json', { oauthAccount: { accountUuid: 'login-b' } });
    assert.equal(await contribution.linkDefault('claude', sources), null, 'a different sign-in without a saved account still blocks the link');
    write('wsl.json', { oauthAccount: { accountUuid: 'login-b' } });
    assert.equal(await contribution.linkDefault('claude', sources), null, 'no environment matched a saved account');
});

const OLD_STATUS_LINE = { type: 'command', command: '"/usr/local/bin/node" "/Users/me/.paddock/config/usage/claude-statusline.cjs" "/Users/me/.paddock/config/usage/claude.json" "/Users/me/.paddock/config/usage/previous-statusline.json"' };

function respond() {
    const reply = { status: 200, value: undefined };
    return { reply, response: { json: data => { reply.value = data; }, status: code => { reply.status = code; return { json: data => { reply.value = data; } }; } } };
}

test('UR09 at start the old Paddock entries are removed from the default Claude settings once, through a link, and the notice is delivered until acknowledged', { skip: process.platform === 'win32' }, async context => {
    const { root, contribution, routes } = fixture(context, home => {
        fs.mkdirSync(path.join(home, 'repo'));
        fs.writeFileSync(path.join(home, 'repo', 'settings.json'), JSON.stringify({ statusLine: OLD_STATUS_LINE, theme: 'dark' }, null, 2));
        fs.mkdirSync(path.join(home, '.claude'));
        fs.symlinkSync(path.join(home, 'repo', 'settings.json'), path.join(home, '.claude', 'settings.json'));
    });
    await contribution.pendingDefaultWrites;
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root, 'repo', 'settings.json'), 'utf8')), { theme: 'dark' });
    assert.equal(fs.lstatSync(path.join(root, '.claude', 'settings.json')).isSymbolicLink(), true);
    const first = respond();
    routes.get('/paddock/legacy-cleanup')({}, first.response);
    assert.equal(first.reply.value.notice.length, 1);
    assert.match(first.reply.value.notice[0], /Removed the old Paddock status line from .*settings\.json \(a link to .*repo.*settings\.json\)\. That repository file changed/);
    const ack = respond();
    routes.get('POST /paddock/legacy-cleanup/ack')({}, ack.response);
    assert.deepEqual(ack.reply.value, { acknowledged: true });
    const second = respond();
    routes.get('/paddock/legacy-cleanup')({}, second.response);
    assert.deepEqual(second.reply.value, { notice: [] });
    // A later start in the same Paddock folder does not look at the files again.
    fs.writeFileSync(path.join(root, 'repo', 'settings.json'), JSON.stringify({ statusLine: OLD_STATUS_LINE }));
    const again = new (contribution.constructor)();
    again.configure({ get() {}, post() {}, delete() {} });
    await again.pendingDefaultWrites;
    assert.equal(JSON.parse(fs.readFileSync(path.join(root, 'repo', 'settings.json'), 'utf8')).statusLine.command, OLD_STATUS_LINE.command);
});

test('UR10 the Paddock settings file is written at start and follows the usage and memory toggles without touching the user settings', async context => {
    const { root, contribution, routes } = fixture(context, home => {
        fs.mkdirSync(path.join(home, '.claude'));
        fs.writeFileSync(path.join(home, '.claude', 'settings.json'), '{"theme":"dark"}\n');
    });
    await contribution.pendingDefaultWrites;
    const flagFile = path.join(root, 'claude', 'flag-settings.json');
    const userFile = path.join(root, '.claude', 'settings.json');
    const userModified = fs.statSync(userFile).mtimeMs;
    assert.deepEqual(JSON.parse(fs.readFileSync(flagFile, 'utf8')), {});
    assert.equal(fs.statSync(path.join(root, 'bin', process.platform === 'win32' ? 'claude.cmd' : 'claude')).isFile(), true);
    const post = async (route, query) => {
        const { reply, response } = respond();
        routes.get(`POST ${route}`)({ query }, response);
        await contribution.pendingDefaultWrites;
        return reply;
    };
    assert.deepEqual((await post('/paddock/usage/claude', { enabled: 'true' })).value, { state: 'on' });
    assert.match(JSON.parse(fs.readFileSync(flagFile, 'utf8')).statusLine.command, /claude-statusline\.cjs/);
    if (process.platform === 'linux') {
        assert.deepEqual((await post('/paddock/memory-notice', { enabled: 'true', minimumMb: '2048' })).value, { state: 'on' });
        assert.match(JSON.parse(fs.readFileSync(flagFile, 'utf8')).hooks.PreToolUse[0].hooks[0].command, /^PADDOCK_MEMORY_MIN_MB=2048 sh /);
    }
    assert.deepEqual((await post('/paddock/usage/claude', { enabled: 'false' })).value, { state: 'off' });
    assert.equal('statusLine' in JSON.parse(fs.readFileSync(flagFile, 'utf8')), false);
    assert.equal((await post('/paddock/usage/claude', {})).status, 400);
    assert.equal(fs.readFileSync(userFile, 'utf8'), '{"theme":"dark"}\n');
    assert.equal(fs.statSync(userFile).mtimeMs, userModified);
    // A restart in the same folder rebuilds the file from the saved choices.
    const restarted = new (contribution.constructor)();
    restarted.configure({ get() {}, post() {}, delete() {} });
    await restarted.pendingDefaultWrites;
    const restartedFlags = JSON.parse(fs.readFileSync(flagFile, 'utf8'));
    assert.equal('statusLine' in restartedFlags, false);
    if (process.platform === 'linux') assert.equal(restartedFlags.hooks.PreToolUse.length, 1);
});

test('UR11 the WSL launcher route reports unsupported without WSL and rejects a missing flag', async context => {
    const { contribution, routes } = fixture(context);
    await contribution.pendingDefaultWrites;
    const missing = respond();
    routes.get('POST /paddock/wsl-claude-path')({ query: {} }, missing.response);
    assert.equal(missing.reply.status, 400);
    const unsupported = respond();
    routes.get('POST /paddock/wsl-claude-path')({ query: { enabled: 'true' } }, unsupported.response);
    await contribution.pendingDefaultWrites;
    assert.deepEqual(unsupported.reply.value, { state: 'unsupported', file: '', isLinked: false });
});

test('UR12 a failing settings-file write at start is reported as a warning and does not skip the launcher install or the old-entry cleanup', { skip: process.platform === 'win32' }, async context => {
    const { root, contribution } = fixture(context, home => {
        fs.mkdirSync(path.join(home, '.claude'));
        fs.writeFileSync(path.join(home, '.claude', 'settings.json'), JSON.stringify({ statusLine: OLD_STATUS_LINE, theme: 'dark' }));
    });
    await contribution.pendingDefaultWrites;
    // Start again from a clean slate with the settings-file write failing.
    fs.rmSync(path.join(root, 'bin'), { recursive: true });
    fs.rmSync(path.join(root, 'usage', 'legacy-cleanup.json'));
    fs.writeFileSync(path.join(root, '.claude', 'settings.json'), JSON.stringify({ statusLine: OLD_STATUS_LINE, theme: 'dark' }));
    context.mock.method(contribution.constructor.prototype, 'writeFlagFiles', async () => { throw new Error('Node.js is needed'); });
    const routes = new Map();
    const restarted = new (contribution.constructor)();
    restarted.configure({ get: (url, route) => routes.set(url, route), post() {}, delete() {} });
    await restarted.pendingDefaultWrites;
    assert.equal(fs.existsSync(path.join(root, 'bin', 'claude')), true, 'the launcher is still installed');
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root, '.claude', 'settings.json'), 'utf8')), { theme: 'dark' }, 'the old entries are still removed');
    const { reply, response } = respond();
    routes.get('/paddock/startup-warnings')({}, response);
    assert.deepEqual(reply.value.warnings, ['Paddock Claude settings could not be written: Node.js is needed']);
});
