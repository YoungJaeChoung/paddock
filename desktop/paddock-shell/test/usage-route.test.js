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
) {
    let Contribution;
    backend.default.registry(token => {
        if (token?.name === 'PaddockStatusRoutes') Contribution = token;
        return { toSelf: () => ({ inSingletonScope() {} }), toService() {}, toConstantValue() {} };
    });
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-usage-route-'));
    const previous = process.env.THEIA_CONFIG_DIR;
    process.env.THEIA_CONFIG_DIR = root;
    context.after(() => { if (previous === undefined) delete process.env.THEIA_CONFIG_DIR; else process.env.THEIA_CONFIG_DIR = previous; fs.rmSync(root, { recursive: true, force: true }); });
    const contribution = new Contribution();
    const routes = new Map();
    contribution.configure({ get: (url, route) => routes.set(url, route), post() {}, delete() {} });
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
