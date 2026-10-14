const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { AccountProfiles, AccountProfile } = require('../account-profiles');
const { AccountUsage, wslFileMapping, claudeLogin, codexLoginId, codexLogin, uniqueLoginMatch, defaultLoginFile } = require('../account-usage');
const usage = require('../usage-model');

function fixture(
    context,
) {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-account-usage-'));
    const accounts = new AccountProfiles({ configDirectory: path.join(directory, 'app-config'), homeDirectory: directory, claudeDirectory: path.join(directory, '.claude'), codexDirectory: path.join(directory, '.codex') });
    const liveCodexUsage = { read: async () => ({ windows: [], updatedAt: null }) };
    const reader = new AccountUsage({ accounts, liveCodexUsage, readWslInfo: async () => { throw new Error('Native usage must not query WSL.'); } });
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    return { directory, accounts, reader, liveCodexUsage };
}

function writeRecord(
    file,
    record,
) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, JSON.stringify(record));
}

function codexRecord(
    used,
) {
    return { type: 'event_msg', payload: { type: 'token_count', rate_limits: { primary: { used_percent: used, window_minutes: 300, resets_at: 1999999999 } } } };
}

test('AU01 a registered account with no records remains empty and lookup creates no usage files', async context => {
    const { accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    assert.equal(fs.existsSync(scope.usageDirectory), false);
    const value = await reader.read(profile.id);
    assert.deepEqual(value, { claude: { state: 'unset', windows: [], updatedAt: null }, codex: { windows: [], updatedAt: null } });
    assert.equal(fs.existsSync(scope.usageDirectory), false);
});

test('AU02 each Claude account reads only its own summary and never fills Codex with a default account', async context => {
    const { directory, accounts, reader } = fixture(context);
    const first = await accounts.create({ provider: 'claude', label: 'First' });
    const second = await accounts.create({ provider: 'claude', label: 'Second' });
    const firstScope = await reader.resolve(first.id);
    const secondScope = await reader.resolve(second.id);
    writeRecord(path.join(directory, '.paddock', 'config', 'usage', 'claude.json'), { rate_limits: { five_hour: { used_percentage: 99 } }, updated_at: 1000 });
    writeRecord(path.join(accounts.configDirectory, 'usage', 'state.json'), { claude: 'on' });
    writeRecord(path.join(firstScope.usageDirectory, 'claude.json'), { rate_limits: { five_hour: { used_percentage: 12, resets_at: 9999 } }, updated_at: 100 });
    writeRecord(path.join(secondScope.usageDirectory, 'claude.json'), { rate_limits: { five_hour: { used_percentage: 43, resets_at: 9999 } }, updated_at: 200 });
    const one = await reader.read(first.id);
    const two = await reader.read(second.id);
    assert.equal(one.claude.windows[0].used, 12);
    assert.equal(two.claude.windows[0].used, 43);
    assert.equal(one.claude.state, 'on');
    assert.deepEqual(one.codex, { windows: [], updatedAt: null });
});

test('AU03 Codex reads its own session limits and never returns conversation content or the other provider', async context => {
    const { accounts, reader } = fixture(context);
    const first = await accounts.create({ provider: 'codex' });
    const second = await accounts.create({ provider: 'codex' });
    const firstScope = await reader.resolve(first.id);
    const secondScope = await reader.resolve(second.id);
    const session = path.join(firstScope.sessionsDirectory, '2026', '10', '05', 'rollout-fixture.jsonl');
    writeRecord(session, codexRecord(21));
    fs.appendFileSync(session, '\n{"type":"message","text":"fixture conversation must stay local"}\n');
    writeRecord(path.join(secondScope.sessionsDirectory, '2026', '10', '05', 'rollout-other.jsonl'), codexRecord(89));
    const value = await reader.read(first.id);
    assert.equal(value.codex.windows[0].used, 21);
    assert.deepEqual(value.claude, { state: 'unset', windows: [], updatedAt: null });
    assert.equal(JSON.stringify(value).includes('fixture conversation'), false);
});

test('AU04 malformed, empty, duplicate-query, and removed identifiers never become default requests', async context => {
    const { accounts, reader } = fixture(context);
    for (const id of ['', undefined, ['first', 'second'], '../.claude', { id: 'anything' }]) {
        await assert.rejects(reader.read(id), error => error.statusCode === 400);
    }
    const profile = await accounts.create({ provider: 'claude' });
    await accounts.remove(profile.id);
    await assert.rejects(reader.read(profile.id), error => error.statusCode === 404);
});

test('AU05 WSL lookup uses the saved distribution and maps spaced Linux homes without preparing a terminal', async context => {
    const setup = fixture(context);
    const profile = { id: '00000000-0000-4000-8000-000000000001', provider: 'codex', label: 'WSL', runtime: 'wsl', wslDistribution: 'Debian Saved' };
    writeRecord(path.join(setup.accounts.configDirectory, AccountProfile.METADATA_FILE), { version: 1, profiles: [profile] });
    const calls = [];
    const reader = new AccountUsage({ ...setup, readWslInfo: async distribution => {
        calls.push(distribution);
        return { ready: true, linuxHome: '/home/person with space', home: '\\\\wsl.localhost\\Debian Saved\\home\\person with space', node: '/usr/bin/node' };
    } });
    const scope = await reader.resolve(profile.id);
    assert.deepEqual(calls, ['Debian Saved']);
    assert.equal(scope.configDir, `/home/person with space/.paddock/agent-profiles/${profile.id}/codex`);
    assert.equal(scope.toLocal(scope.sessionsDirectory), `\\\\wsl.localhost\\Debian Saved\\home\\person with space\\.paddock\\agent-profiles\\${profile.id}\\codex\\sessions`);
    assert.equal(fs.existsSync(path.join(setup.accounts.configDirectory, 'agent-profiles')), false);
    assert.throws(() => wslFileMapping({ ready: true, linuxHome: '/home/user', home: '\\\\wsl.localhost\\Ubuntu\\home\\user' }, 'Debian Saved'), error => error.statusCode === 503);
    assert.throws(() => wslFileMapping({ ready: false }, 'Debian Saved'), error => error.statusCode === 503);
});

test('AU08 the account status script saves only limit fields in that account folder, found through CLAUDE_CONFIG_DIR', async context => {
    const { accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    const defaultFile = path.join(accounts.configDirectory, 'usage', 'claude.json');
    const result = spawnSync(process.execPath, [path.join(__dirname, '..', 'claude-statusline.cjs'), defaultFile], {
        encoding: 'utf8', env: { PATH: process.env.PATH, CLAUDE_CONFIG_DIR: scope.configDir },
        input: JSON.stringify({ model: { display_name: 'Fixture' }, session_id: 'fixture-private-field', rate_limits: { five_hour: { used_percentage: 37, resets_at: 1999999999 } } }),
    });
    assert.equal(result.status, 0, result.stderr);
    assert.equal((await reader.read(profile.id)).claude.windows[0].used, 37);
    const record = JSON.parse(fs.readFileSync(path.join(scope.usageDirectory, 'claude.json'), 'utf8'));
    assert.deepEqual(Object.keys(record).sort(), ['rate_limits', 'updated_at']);
    assert.equal(fs.existsSync(defaultFile), false, 'an account never writes the default usage record');
});
test('AU10 redirected usage files and profile directories are rejected', { skip: process.platform === 'win32' }, async context => {
    const { directory, accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    const outside = path.join(directory, 'outside.json');
    fs.writeFileSync(outside, '{}');
    fs.mkdirSync(scope.usageDirectory);
    const usageFile = path.join(scope.usageDirectory, 'claude.json');
    fs.symlinkSync(outside, usageFile);
    await assert.rejects(reader.read(profile.id), /redirected/);
    assert.equal(fs.readFileSync(outside, 'utf8'), '{}');
});
test('AU11 a Codex scan does not follow a session file link outside the account', { skip: process.platform === 'win32' }, async context => {
    const { directory, accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'codex' });
    const scope = await reader.resolve(profile.id);
    const outside = path.join(directory, 'unrelated.jsonl');
    writeRecord(outside, codexRecord(95));
    const day = path.join(scope.sessionsDirectory, '2026', '10', '05');
    fs.mkdirSync(day, { recursive: true });
    fs.symlinkSync(outside, path.join(day, 'rollout-linked.jsonl'));
    assert.deepEqual((await reader.read(profile.id)).codex, { windows: [], updatedAt: null });
});

test('AU13 malformed or unreadable usage records are unavailable instead of silently empty', async context => {
    const { accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    const record = path.join(scope.usageDirectory, 'claude.json');
    fs.mkdirSync(scope.usageDirectory);
    fs.writeFileSync(record, '{unfinished');
    await assert.rejects(reader.read(profile.id), error => error.statusCode === 503);
    writeRecord(record, { rate_limits: {} });
    const originalRead = fs.readFileSync;
    context.mock.method(fs, 'readFileSync', (file, ...argumentsList) => {
        if (file === record) throw Object.assign(new Error('fixture denied'), { code: 'EACCES' });
        return originalRead(file, ...argumentsList);
    });
    await assert.rejects(reader.read(profile.id), error => error.statusCode === 503);
});

test('AU16 POSIX status-line paths preserve shell characters during execution', { skip: process.platform === 'win32' }, context => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-status-quote-'));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    const folder = path.join(directory, 'app $(touch PWNED) `touch PWNED2` "quoted"');
    fs.mkdirSync(folder);
    const script = path.join(folder, 'claude-statusline.cjs');
    fs.copyFileSync(path.join(__dirname, '..', 'claude-statusline.cjs'), script);
    const command = [process.execPath, script, path.join(folder, 'claude.json')].map(file => usage.quoteStatusLineArgument(file)).join(' ');
    assert.equal(usage.paddockStatusLineFiles(command).script, script);
    const result = spawnSync(command, { shell: true, cwd: directory, env: { PATH: process.env.PATH, CLAUDE_CONFIG_DIR: path.join(directory, 'none') }, input: JSON.stringify({ model: { display_name: 'Fixture' }, rate_limits: { five_hour: { used_percentage: 17, resets_at: 1999999999 } } }), encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, 'Fixture');
    assert.equal(fs.existsSync(path.join(directory, 'PWNED')), false);
    assert.equal(fs.existsSync(path.join(directory, 'PWNED2')), false);
    assert.equal(JSON.parse(fs.readFileSync(path.join(folder, 'claude.json'), 'utf8')).rate_limits.five_hour.used_percentage, 17);
});
test('AU17 a Codex usage scan stops after two recent day folders', async context => {
    const { accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'codex' });
    const scope = await reader.resolve(profile.id);
    for (const day of ['01', '02', '03']) writeRecord(path.join(scope.sessionsDirectory, '2026', '10', day, 'rollout-fixture.jsonl'), codexRecord(10));
    const scanned = [];
    const originalRead = fs.readdirSync;
    context.mock.method(fs, 'readdirSync', (directory, ...argumentsList) => {
        scanned.push(directory);
        return originalRead(directory, ...argumentsList);
    });
    await reader.read(profile.id);
    assert.equal(scanned.includes(path.join(scope.sessionsDirectory, '2026', '10', '01')), false);
});


test('AU18 Codex official usage works without a sessions folder and receives only its own account scope', async context => {
    const setup = fixture(context);
    const profile = await setup.accounts.create({ provider: 'codex' });
    const scope = await setup.reader.resolve(profile.id);
    const calls = [];
    const expected = { windows: [{ label: 'week', used: 83, resetsAt: 1999999999 }], updatedAt: 100 };
    setup.liveCodexUsage.read = async input => { calls.push(input); return expected; };
    assert.equal(fs.existsSync(scope.sessionsDirectory), false);
    assert.deepEqual((await setup.reader.read(profile.id)).codex, expected);
    assert.equal(calls[0].configDir, scope.configDir);
    assert.equal(calls[0].profile.id, profile.id);
});

test('AU19 Codex live failures use available legacy limits but never turn an empty lookup into zero usage', async context => {
    const setup = fixture(context);
    const profile = await setup.accounts.create({ provider: 'codex' });
    const scope = await setup.reader.resolve(profile.id);
    setup.liveCodexUsage.read = async () => { throw Object.assign(new Error('Codex usage unavailable'), { statusCode: 503 }); };
    await assert.rejects(setup.reader.read(profile.id), error => error.statusCode === 503);
    writeRecord(path.join(scope.sessionsDirectory, '2026', '10', '05', 'rollout-fixture.jsonl'), codexRecord(21));
    assert.equal((await setup.reader.read(profile.id)).codex.windows[0].used, 21);
});

test('AU20 the default Claude sign-in is linked only to the saved account with the same account number', async context => {
    const { directory, accounts, reader } = fixture(context);
    const main = await accounts.create({ provider: 'claude', label: 'Main' });
    const other = await accounts.create({ provider: 'claude', label: 'Other' });
    await accounts.create({ provider: 'codex', label: 'Codex' });
    const mainScope = await reader.resolve(main.id);
    writeRecord(path.join((await reader.resolve(other.id)).configDir, '.claude.json'), { oauthAccount: { accountUuid: 'other-login' } });
    writeRecord(path.join(mainScope.configDir, '.claude.json'), { oauthAccount: { accountUuid: 'main-login', emailAddress: 'main@example.com' } });
    writeRecord(path.join(mainScope.usageDirectory, 'claude.json'), { rate_limits: { five_hour: { used_percentage: 30 } }, updated_at: 500 });
    const match = await reader.matchClaudeLogin('main-login');
    assert.equal(match.profile.label, 'Main');
    assert.equal(match.record.updated_at, 500);
    assert.equal(await reader.matchClaudeLogin('unknown-login'), null);
    fs.writeFileSync(path.join(directory, 'half-written.json'), '{"oauthAccount":');
    assert.equal(claudeLogin(path.join(directory, 'half-written.json')), null);
    assert.equal(claudeLogin(path.join(directory, 'missing.json')), null);
});

/** 서명 없는 가짜 id_token. 본문(두 번째 조각)만 의미가 있다. */
function fakeIdToken(
    claims,
) {
    return ['e30', Buffer.from(JSON.stringify(claims)).toString('base64url'), 'signature'].join('.');
}

test('AU21 Codex 로그인 식별자는 계정 id(와 사용자 id)만 꺼내고 토큰은 돌려주지 않는다', () => {
    assert.equal(codexLoginId({ tokens: { account_id: 'acc-1', access_token: 'secret-access', refresh_token: 'secret-refresh' } }), 'acc-1');
    assert.equal(codexLoginId({ tokens: { account_id: 'acc-1', id_token: fakeIdToken({ sub: 'u-1' }) } }), 'acc-1/u-1');
    assert.equal(codexLoginId({ tokens: { id_token: fakeIdToken({ 'https://api.openai.com/auth': { chatgpt_account_id: 'acc-2', chatgpt_user_id: 'u-2' } }) } }), 'acc-2/u-2');
    assert.equal(codexLoginId({ tokens: { id_token: fakeIdToken({ 'https://api.openai.com/auth': { chatgpt_account_id: 'acc-3' } }) } }), 'acc-3');
    assert.equal(codexLoginId({ tokens: { account_id: 'acc-4', id_token: 'not-a-jwt' } }), 'acc-4');
    assert.equal(codexLoginId({ OPENAI_API_KEY: 'sk-secret' }), null);
    assert.equal(codexLoginId({ tokens: { account_id: 42 } }), null);
    assert.equal(codexLoginId(null), null);
});

test('AU22 로그인이 같은 등록 계정이 정확히 하나일 때만 연결하고, 없거나 둘 이상이면 연결하지 않는다', () => {
    const A = { id: 'a' };
    const B = { id: 'b' };
    assert.equal(uniqueLoginMatch('x', [{ profile: A, login: 'x' }, { profile: B, login: 'y' }]), A);
    assert.equal(uniqueLoginMatch('x', [{ profile: A, login: 'x' }, { profile: B, login: 'x' }]), null);
    assert.equal(uniqueLoginMatch('x', [{ profile: A, login: 'y' }, { profile: B, login: null }]), null);
    assert.equal(uniqueLoginMatch(null, [{ profile: A, login: null }]), null);
    assert.equal(uniqueLoginMatch('', []), null);
});

test('AU23 기본 CLI의 로그인 파일은 실행 환경의 계정 변수로 정한다', () => {
    const join = path.posix.join;
    assert.equal(defaultLoginFile('claude', { HOME: '/home/me' }, join), '/home/me/.claude.json');
    assert.equal(defaultLoginFile('claude', { HOME: '/home/me', CLAUDE_CONFIG_DIR: '/c' }, join), '/c/.claude.json');
    assert.equal(defaultLoginFile('codex', { HOME: '/home/me' }, join), '/home/me/.codex/auth.json');
    assert.equal(defaultLoginFile('codex', { HOME: '/home/me', CODEX_HOME: '/x' }, join), '/x/auth.json');
    assert.equal(defaultLoginFile('codex', {}, join), null);
    assert.equal(defaultLoginFile('bash', { HOME: '/home/me' }, join), null);
});

test('AU24 기본 Codex 로그인은 같은 로그인의 등록 Codex 계정 하나에만 연결되고, 같은 로그인이 둘이면 모호해 연결하지 않는다', async context => {
    const { directory, accounts, reader } = fixture(context);
    const main = await accounts.create({ provider: 'codex', label: 'Main' });
    const other = await accounts.create({ provider: 'codex', label: 'Other' });
    const claude = await accounts.create({ provider: 'claude', label: 'Claude' });
    writeRecord(path.join((await reader.resolve(main.id)).configDir, 'auth.json'), { tokens: { account_id: 'acc-main', access_token: 'secret' } });
    writeRecord(path.join((await reader.resolve(other.id)).configDir, 'auth.json'), { tokens: { account_id: 'acc-other' } });
    writeRecord(path.join((await reader.resolve(claude.id)).configDir, '.claude.json'), { oauthAccount: { accountUuid: 'acc-main' } });
    writeRecord(path.join(directory, '.codex', 'auth.json'), { tokens: { account_id: 'acc-main' } });
    const login = codexLogin(path.join(directory, '.codex', 'auth.json'));
    assert.equal(login, 'acc-main');
    assert.equal((await reader.matchLogin({ provider: 'codex', login, runtime: 'native' })).id, main.id, '다른 도구의 같은 문자열 id는 대조하지 않는다');
    assert.equal(await reader.matchLogin({ provider: 'codex', login, runtime: 'wsl', wslDistribution: 'Ubuntu' }), null, '다른 실행 환경의 계정은 대조하지 않는다');
    assert.equal(await reader.matchLogin({ provider: 'codex', login: 'acc-unknown', runtime: 'native' }), null);
    assert.equal(await reader.matchLogin({ provider: 'codex', login: null, runtime: 'native' }), null);
    assert.equal(codexLogin(path.join(directory, 'missing.json')), null);

    // 같은 로그인을 두 등록 계정이 쓰면 어느 쪽인지 단정할 수 없다. 새 읽기(캐시 없음)로 확인한다.
    writeRecord(path.join((await reader.resolve(other.id)).configDir, 'auth.json'), { tokens: { account_id: 'acc-main' } });
    const fresh = new AccountUsage({ accounts, readWslInfo: async () => { throw new Error('unused'); } });
    assert.equal(await fresh.matchLogin({ provider: 'codex', login, runtime: 'native' }), null);
});

test('AU25 같은 로그인의 등록 Claude 계정이 둘이면 기본 Claude를 어느 쪽에도 연결하지 않는다', async context => {
    const { accounts, reader } = fixture(context);
    const first = await accounts.create({ provider: 'claude', label: 'First' });
    const second = await accounts.create({ provider: 'claude', label: 'Second' });
    for (const profile of [first, second]) writeRecord(path.join((await reader.resolve(profile.id)).configDir, '.claude.json'), { oauthAccount: { accountUuid: 'same-login' } });
    assert.equal(await reader.matchClaudeLogin('same-login'), null);
    assert.equal(await reader.matchClaudeLogin('same-login', { runtime: 'native' }), null);
});

test('AU26 계정을 추가·삭제하면 로그인 캐시가 남아 있어도 다음 대조에 바로 반영된다', async context => {
    const { accounts, reader } = fixture(context);
    const main = await accounts.create({ provider: 'codex', label: 'Main' });
    writeRecord(path.join((await reader.resolve(main.id)).configDir, 'auth.json'), { tokens: { account_id: 'acc-main' } });
    const request = { provider: 'codex', login: 'acc-main', runtime: 'native' };
    assert.equal((await reader.matchLogin(request)).id, main.id);
    // 같은 로그인의 두 번째 계정을 등록하면, 캐시 시간이 지나기 전이어도 모호해져 연결하지 않는다.
    const second = await accounts.create({ provider: 'codex', label: 'Second' });
    writeRecord(path.join((await reader.resolve(second.id)).configDir, 'auth.json'), { tokens: { account_id: 'acc-main' } });
    assert.equal(await reader.matchLogin(request), null);
    await accounts.remove(second.id);
    assert.equal((await reader.matchLogin(request)).id, main.id);
    assert.equal(reader.savedLoginCache.has(second.id), false, '지운 계정의 캐시는 남지 않는다');
});

