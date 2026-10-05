const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { AccountProfiles } = require('../account-profiles');
const { AccountSessions, findClaudeTranscript, readClaudeSession, chooseAgentProcess } = require('../account-session');

const SESSION_ID = '00000000-0000-4000-8000-000000000011';

function fixture(
    context,
) {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-session-'));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    const configDir = path.join(directory, 'claude');
    fs.mkdirSync(path.join(configDir, 'sessions'), { recursive: true });
    fs.mkdirSync(path.join(configDir, 'projects', 'project'), { recursive: true });
    return { directory, configDir, join: path.join, toLocal: file => file };
}

test('C-ACCOUNT-RESUME-B01 identifies a Claude record by live PID and process start, not the newest file', context => {
    const scope = fixture(context);
    const record = { pid: 42, procStart: '700', sessionId: SESSION_ID, cwd: '/work', kind: 'interactive' };
    fs.writeFileSync(path.join(scope.configDir, 'sessions', '42.json'), JSON.stringify(record));
    fs.writeFileSync(path.join(scope.configDir, 'sessions', '43.json'), JSON.stringify({ ...record, pid: 43, sessionId: '00000000-0000-4000-8000-000000000022' }));
    assert.deepEqual(readClaudeSession(scope, { pid: 42, start: '700' }), { sessionId: SESSION_ID, cwd: '/work' });
    assert.equal(readClaudeSession(scope, { pid: 42, start: '701' }), null);
    assert.equal(readClaudeSession(scope, { pid: 44, start: '700' }), null);
});

test('C-ACCOUNT-RESUME-B02 scans exact transcript filenames without reading conversation contents', context => {
    const scope = fixture(context);
    const transcript = path.join(scope.configDir, 'projects', 'project', `${SESSION_ID}.jsonl`);
    fs.writeFileSync(transcript, 'fixture conversation; this is deliberately not valid JSON');
    fs.writeFileSync(path.join(scope.configDir, 'projects', 'project', 'newer.jsonl'), 'newer but unrelated');
    assert.equal(findClaudeTranscript(scope, SESSION_ID), transcript);
    fs.mkdirSync(path.join(scope.configDir, 'projects', 'other'));
    fs.writeFileSync(path.join(scope.configDir, 'projects', 'other', `${SESSION_ID}.jsonl`), 'duplicate');
    assert.throws(() => findClaudeTranscript(scope, SESSION_ID), /more than one/);
});

test('C-ACCOUNT-RESUME-B03 refuses transcript and registry symlinks', { skip: process.platform === 'win32' }, context => {
    const scope = fixture(context);
    const outside = path.join(scope.directory, 'outside.json');
    fs.writeFileSync(outside, '{}');
    fs.symlinkSync(outside, path.join(scope.configDir, 'sessions', '42.json'));
    assert.throws(() => readClaudeSession(scope, { pid: 42, start: '700' }), /redirected/);
    fs.symlinkSync(outside, path.join(scope.configDir, 'projects', 'project', `${SESSION_ID}.jsonl`));
    assert.throws(() => findClaudeTranscript(scope, SESSION_ID), /redirected/);
});

test('C-ACCOUNT-RESUME-B04 chooses only one topmost matching agent below the terminal shell', () => {
    const processes = [
        { pid: 10, parentPid: 1, start: '10', argv: ['bash'] },
        { pid: 20, parentPid: 10, start: '20', argv: ['node', '/bin/claude'] },
        { pid: 30, parentPid: 20, start: '30', argv: ['claude'] },
        { pid: 40, parentPid: 1, start: '40', argv: ['claude'] },
    ];
    assert.equal(chooseAgentProcess(processes, 10, 'claude')?.pid, 20);
    assert.equal(chooseAgentProcess([...processes, { pid: 50, parentPid: 10, start: '50', argv: ['claude'] }], 10, 'claude'), null);
});

test('C-ACCOUNT-RESUME-B05 rejects unsupported providers and malformed identities before filesystem lookup', async () => {
    const sessions = new AccountSessions({ accounts: {} });
    await assert.rejects(sessions.prepareResume('target', { terminalId: 'term', shellPid: 12, provider: 'codex', runtime: 'native' }), /Codex/);
    await assert.rejects(sessions.observeSession({ terminalId: '../term', shellPid: 12, provider: 'claude', runtime: 'native' }), /terminal identifier/);
    await assert.rejects(sessions.observeSession({ terminalId: 'term', shellPid: -1, provider: 'claude', runtime: 'native' }), /process identifier/);
});

async function eventually(
    predicate,
) {
    const deadline = Date.now() + 5000;
    let result = null;
    while (!result && Date.now() < deadline) {
        result = await predicate();
        if (!result) await new Promise(resolve => setTimeout(resolve, 25));
    }
    assert.ok(result, 'The fixture did not reach its expected state.');
    return result;
}

async function runningFixture(
    context,
) {
    const files = fixture(context);
    const accounts = new AccountProfiles({ configDirectory: path.join(files.directory, 'config'), homeDirectory: files.directory, claudeDirectory: files.configDir });
    const source = await accounts.create({ provider: 'claude', label: 'Source', runtime: 'native' });
    const target = await accounts.create({ provider: 'claude', label: 'Target', runtime: 'native' });
    const prepared = await accounts.prepare(source.id);
    const script = path.join(files.directory, 'claude.cjs');
    fs.writeFileSync(script, `const fs = require('node:fs'); const path = require('node:path');
const config = process.env.CLAUDE_CONFIG_DIR;
const registry = path.join(config, 'sessions');
const project = path.join(config, 'projects', 'synthetic');
fs.mkdirSync(registry, { recursive: true }); fs.mkdirSync(project, { recursive: true });
const procStart = fs.readFileSync('/proc/' + process.pid + '/stat', 'utf8').split(') ')[1].split(' ')[19];
const record = { pid: process.pid, procStart, sessionId: '${SESSION_ID}', cwd: process.cwd(), kind: 'interactive' };
fs.writeFileSync(path.join(project, '${SESSION_ID}.jsonl'), 'synthetic conversation; no real user text');
fs.writeFileSync(path.join(registry, process.pid + '.json'), JSON.stringify(record));
process.on('SIGTERM', () => { fs.unlinkSync(path.join(registry, process.pid + '.json')); process.exit(0); });
setInterval(() => {}, 1000);
`);
    const shell = spawn('/bin/bash', ['--noprofile', '--norc'], {
        cwd: files.directory, env: { PATH: process.env.PATH, CLAUDE_CONFIG_DIR: prepared.configDir }, stdio: ['pipe', 'ignore', 'ignore'],
    });
    let childPid;
    context.after(() => {
        if (childPid) { try { process.kill(childPid, 'SIGTERM'); } catch {} }
        shell.stdin.end('exit\n');
        shell.kill('SIGTERM');
    });
    shell.stdin.write(`'${process.execPath.replace(/'/g, `'"'"'`)}' '${script.replace(/'/g, `'"'"'`)}'\n`);
    childPid = await eventually(() => {
        let records = [];
        try { records = fs.readdirSync(path.join(prepared.configDir, 'sessions')); } catch {}
        return records.length ? Number(records[0].replace('.json', '')) : null;
    });
    const sessions = new AccountSessions({ accounts });
    const request = { terminalId: 'synthetic-terminal', shellPid: shell.pid, accountId: source.id, provider: 'claude', runtime: 'native' };
    return { ...files, accounts, source, target, prepared, shell, childPid, sessions, request };
}

test('C-ACCOUNT-RESUME-B06 observes a live CLI, prepares the same ID, stops only that CLI, and retains the stopped conversation', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    const snapshot = await setup.sessions.observeSession(setup.request);
    assert.equal(snapshot.sessionId, SESSION_ID);
    assert.equal(snapshot.sourceAccountId, setup.source.id);
    const prepared = await setup.sessions.prepareResume(setup.target.id, setup.request);
    assert.equal(prepared.profile.id, setup.target.id);
    assert.deepEqual(prepared.resume, snapshot);
    assert.equal(fs.existsSync(path.join(setup.prepared.configDir, 'sessions', `${setup.childPid}.json`)), true);
    const stopped = await setup.sessions.stopSession(setup.request);
    assert.equal(stopped.stopped, true);
    assert.equal(fs.existsSync(path.join(setup.prepared.configDir, 'sessions', `${setup.childPid}.json`)), false);
    assert.equal(setup.shell.exitCode, null);
    assert.deepEqual(await setup.sessions.observeSession(setup.request), snapshot);
});

test('C-ACCOUNT-RESUME-B07 follows a changed conversation ID and rejects stopping stale preparation', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    await setup.sessions.prepareResume(setup.target.id, setup.request);
    const registry = path.join(setup.prepared.configDir, 'sessions', `${setup.childPid}.json`);
    const record = JSON.parse(fs.readFileSync(registry, 'utf8'));
    const nextId = '00000000-0000-4000-8000-000000000033';
    fs.writeFileSync(path.join(setup.prepared.configDir, 'projects', 'synthetic', `${nextId}.jsonl`), 'new synthetic conversation');
    fs.writeFileSync(registry, JSON.stringify({ ...record, sessionId: nextId }));
    await assert.rejects(setup.sessions.stopSession(setup.request), /conversation changed/);
    assert.equal((await setup.sessions.observeSession(setup.request)).sessionId, nextId);
    assert.equal(fs.existsSync(`/proc/${setup.childPid}`), true);
});

test('C-ACCOUNT-RESUME-B08 refuses another active command and does not kill it', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    await setup.sessions.prepareResume(setup.target.id, setup.request);
    await setup.sessions.stopSession(setup.request);
    setup.shell.stdin.write('sleep 60\n');
    const unrelatedPid = await eventually(() => {
        const children = fs.readFileSync(`/proc/${setup.shell.pid}/task/${setup.shell.pid}/children`, 'utf8').trim();
        return children ? Number(children.split(' ')[0]) : null;
    });
    context.after(() => { try { process.kill(unrelatedPid, 'SIGTERM'); } catch {} });
    await assert.rejects(setup.sessions.stopSession(setup.request), /Another command/);
    assert.equal(fs.existsSync(`/proc/${unrelatedPid}`), true);
    assert.equal(await setup.sessions.observeSession(setup.request), undefined);
});

test('C-ACCOUNT-RESUME-B09 reconstructs a restored path from its source account and ignores a supplied path', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    const snapshot = await setup.sessions.observeSession(setup.request);
    await setup.sessions.prepareResume(setup.target.id, setup.request);
    await setup.sessions.stopSession(setup.request);
    const restored = new AccountSessions({ accounts: setup.accounts });
    const actual = await restored.observeSession({ ...setup.request, resume: { ...snapshot, transcriptPath: '/outside/credentials.json' } });
    assert.equal(actual.transcriptPath, snapshot.transcriptPath);
    assert.equal(actual.sourceAccountId, setup.source.id);
});

test('C-ACCOUNT-RESUME-B10 does not guess a conversation for an unobserved stopped terminal', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    await setup.sessions.prepareResume(setup.target.id, setup.request);
    await setup.sessions.stopSession(setup.request);
    const unknown = new AccountSessions({ accounts: setup.accounts });
    assert.equal(await unknown.observeSession(setup.request), null);
    await assert.rejects(unknown.prepareResume(setup.target.id, setup.request), /could not be identified/);
});

test('C-ACCOUNT-RESUME-B11 refuses a changed conversation even when background observation already saw the change', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    await setup.sessions.prepareResume(setup.target.id, setup.request);
    const registry = path.join(setup.prepared.configDir, 'sessions', `${setup.childPid}.json`);
    const record = JSON.parse(fs.readFileSync(registry, 'utf8'));
    const nextId = '00000000-0000-4000-8000-000000000044';
    fs.writeFileSync(path.join(setup.prepared.configDir, 'projects', 'synthetic', `${nextId}.jsonl`), 'new synthetic conversation');
    fs.writeFileSync(registry, JSON.stringify({ ...record, sessionId: nextId }));
    assert.equal((await setup.sessions.observeSession(setup.request)).sessionId, nextId);
    await assert.rejects(setup.sessions.stopSession(setup.request), /conversation changed/);
    assert.equal(fs.existsSync(`/proc/${setup.childPid}`), true);
});

test('C-ACCOUNT-RESUME-B12 permits retry after a verified stop when failed replacement already closed the original shell', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    const prepared = await setup.sessions.prepareResume(setup.target.id, setup.request);
    await setup.sessions.stopSession(setup.request);
    setup.shell.stdin.end('exit\n');
    await eventually(() => setup.shell.exitCode === 0);
    const retry = await setup.sessions.prepareResume(setup.target.id, { ...setup.request, resume: prepared.resume });
    assert.deepEqual(retry.resume, prepared.resume);
    assert.equal((await setup.sessions.stopSession(setup.request)).stopped, true);
});

test('C-ACCOUNT-RESUME-B13 never revives an old snapshot after a new unsaved conversation was observed', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    const old = await setup.sessions.observeSession(setup.request);
    const registry = path.join(setup.prepared.configDir, 'sessions', `${setup.childPid}.json`);
    const record = JSON.parse(fs.readFileSync(registry, 'utf8'));
    fs.writeFileSync(registry, JSON.stringify({ ...record, sessionId: '00000000-0000-4000-8000-000000000055' }));
    assert.equal(await setup.sessions.observeSession({ ...setup.request, resume: old }), null);
    process.kill(setup.childPid, 'SIGTERM');
    await eventually(() => !fs.existsSync(registry));
    await eventually(() => fs.readFileSync(`/proc/${setup.shell.pid}/task/${setup.shell.pid}/children`, 'utf8').trim() === '');
    assert.equal(await setup.sessions.observeSession({ ...setup.request, resume: old }), null);
    await assert.rejects(setup.sessions.prepareResume(setup.target.id, { ...setup.request, resume: old }), /could not be identified/);
});

test('C-ACCOUNT-RESUME-B14 keeps the original account path through a new account registry startup gap', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    const original = await setup.sessions.observeSession(setup.request);
    const target = await setup.accounts.prepare(setup.target.id);
    const otherBackend = new AccountSessions({ accounts: setup.accounts });
    const request = { ...setup.request, accountId: setup.target.id, resume: original };
    assert.equal(await otherBackend.observeSession(request), undefined);
    await assert.rejects(otherBackend.prepareResume(setup.source.id, request), /could not be identified/);
    fs.mkdirSync(path.join(target.configDir, 'sessions'), { recursive: true });
    fs.copyFileSync(path.join(setup.prepared.configDir, 'sessions', `${setup.childPid}.json`), path.join(target.configDir, 'sessions', `${setup.childPid}.json`));
    const actual = await otherBackend.observeSession(request);
    assert.equal(actual.sourceAccountId, setup.source.id);
    assert.equal(actual.transcriptPath, original.transcriptPath);
    assert.equal(fs.existsSync(path.join(target.configDir, 'projects')), false);
});

test('C-ACCOUNT-RESUME-B15 refuses an idle restore with a different process start identity', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    const prepared = await setup.sessions.prepareResume(setup.target.id, setup.request);
    await setup.sessions.stopSession(setup.request);
    const restored = new AccountSessions({ accounts: setup.accounts });
    const request = { ...setup.request, resume: { ...prepared.resume, shellStart: `${prepared.resume.shellStart}-reused` } };
    assert.equal(await restored.observeSession(request), undefined);
});

test('C-ACCOUNT-RESUME-B16 rejects account and saved snapshot environment changes before stopping a process', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    const codex = await setup.accounts.create({ provider: 'codex', label: 'Other provider', runtime: 'native' });
    await assert.rejects(setup.sessions.prepareResume(codex.id, setup.request), /same CLI and environment/);
    const snapshot = await setup.sessions.observeSession(setup.request);
    await assert.rejects(setup.sessions.prepareResume(setup.target.id, { ...setup.request, resume: { ...snapshot, runtime: 'wsl', wslDistribution: 'Other' } }), /another CLI or environment/);
    assert.equal(fs.existsSync(`/proc/${setup.childPid}`), true);
});

test('C-ACCOUNT-RESUME-B17 one removed account does not block other terminal observations', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    const removed = { ...setup.request, terminalId: 'removed-terminal', accountId: '00000000-0000-4000-8000-000000000099' };
    const snapshots = await setup.sessions.observeSessions([removed, setup.request]);
    assert.equal(Object.hasOwn(snapshots, removed.terminalId), false);
    assert.equal(snapshots[setup.request.terminalId].sessionId, SESSION_ID);
    await assert.rejects(setup.sessions.observeSession(removed), /source account was removed/);
});

test('C-ACCOUNT-RESUME-B18 a known missing transcript preserves the restore binding and prevents stopping or starting a fresh conversation', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    const prepared = await setup.sessions.prepareResume(setup.target.id, setup.request);
    fs.unlinkSync(prepared.resume.transcriptPath);
    await assert.rejects(setup.sessions.stopSession(setup.request), /saved conversation file is missing/);
    const request = { ...setup.request, resume: prepared.resume };
    const snapshots = await setup.sessions.observeSessions([request]);
    assert.equal(Object.hasOwn(snapshots, request.terminalId), false);
    await assert.rejects(setup.sessions.prepareResume(setup.target.id, request), /saved conversation file is missing/);
    assert.equal(fs.existsSync(`/proc/${setup.childPid}`), true);
});

test('C-ACCOUNT-RESUME-B19 an idle terminal also requires its saved transcript to still exist', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    const prepared = await setup.sessions.prepareResume(setup.target.id, setup.request);
    await setup.sessions.stopSession(setup.request);
    fs.unlinkSync(prepared.resume.transcriptPath);
    assert.equal(await setup.sessions.observeSession({ ...setup.request, resume: prepared.resume }), undefined);
    await assert.rejects(setup.sessions.prepareResume(setup.target.id, { ...setup.request, resume: prepared.resume }), /saved conversation file is missing/);
    const restored = new AccountSessions({ accounts: setup.accounts });
    assert.equal(await restored.observeSession({ ...setup.request, resume: prepared.resume }), undefined);
});

test('C-ACCOUNT-RESUME-B20 a replacement shell startup gap preserves the requested conversation without authorizing resume yet', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    const prepared = await setup.sessions.prepareResume(setup.target.id, setup.request);
    await setup.sessions.stopSession(setup.request);
    const backend = new AccountSessions({ accounts: setup.accounts });
    const request = { ...setup.request, accountId: setup.target.id, resume: { ...prepared.resume, shellPid: setup.request.shellPid + 1000 } };
    assert.equal(await backend.observeSession(request), undefined);
    assert.equal(await backend.observeSession(request), undefined);
    await assert.rejects(backend.prepareResume(setup.source.id, request), /could not be identified/);
    const snapshots = await backend.observeSessions([request]);
    assert.equal(Object.hasOwn(snapshots, request.terminalId), false);
});

test('C-ACCOUNT-RESUME-B21 a Claude running with another configuration folder is not matched against the default records', { skip: process.platform !== 'linux' }, async context => {
    const setup = await runningFixture(context);
    // 일반 터미널에서 CLAUDE_CONFIG_DIR로 계정 폴더를 지정한 Claude를 기본 범위(accountId 없음)로 물으면 대화를 추측하지 않는다.
    const request = { ...setup.request, accountId: null };
    assert.equal(await setup.sessions.observeSession(request), undefined);
    await assert.rejects(setup.sessions.prepareResume(setup.target.id, request), /different configuration folder/);
    assert.equal((await setup.sessions.observeSession(setup.request)).sessionId, SESSION_ID);
});
