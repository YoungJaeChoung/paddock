const test = require('node:test');
const assert = require('node:assert/strict');
const { claudeSessionOf, codexSessionHeader, chooseCodexSession, listCodexSessions, elapsedSeconds, resumableAgent } = require('../agent-resume');

const CLAUDE_ID = '0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10';
const CODEX_ID = '01a0ae67-7d1b-7ba0-a75e-113eb195b380';

const rollout = (overrides = {}) => JSON.stringify({ payload: { id: CODEX_ID, cwd: '/work/app', originator: 'codex-tui', source: 'cli', timestamp: '2026-10-14T01:00:10.000Z', ...overrides } });

test('claudeSessionOf: pid가 같은 대화형 기록만 인정한다', () => {
    const record = (extra = {}) => JSON.stringify({ pid: 42, sessionId: CLAUDE_ID, kind: 'interactive', cwd: '/work/app', ...extra });
    const read = text => () => text;
    assert.deepEqual(claudeSessionOf({ configDirectory: '/c', pid: 42, readFile: read(record()) }), { sessionId: CLAUDE_ID, cwd: '/work/app' });
    assert.equal(claudeSessionOf({ configDirectory: '/c', pid: 43, readFile: read(record()) }), null);
    assert.equal(claudeSessionOf({ configDirectory: '/c', pid: 42, readFile: read(record({ kind: 'sdk' })) }), null);
    assert.equal(claudeSessionOf({ configDirectory: '/c', pid: 42, readFile: read(record({ sessionId: 'not-a-uuid' })) }), null);
    assert.equal(claudeSessionOf({ configDirectory: '/c', pid: 42, readFile: read('{broken') }), null);
    assert.equal(claudeSessionOf({ configDirectory: '/c', pid: 42, readFile: () => { throw new Error('ENOENT'); } }), null);
});

test('claudeSessionOf: 계정 폴더 아래 sessions/<pid>.json을 읽는다', () => {
    let requested = '';
    claudeSessionOf({ configDirectory: '/acc', pid: 7, readFile: file => { requested = file; return ''; } });
    assert.equal(requested, '/acc/sessions/7.json');
});

test('codexSessionHeader: 터미널에서 직접 연 대화만 인정한다', () => {
    assert.deepEqual(codexSessionHeader(rollout()), { id: CODEX_ID, cwd: '/work/app', startedAt: Date.parse('2026-10-14T01:00:10.000Z') });
    assert.equal(codexSessionHeader(rollout({ originator: 'codex_exec', source: 'exec' })), null);
    assert.equal(codexSessionHeader(rollout({ source: { subagent: {} } })), null);
    assert.equal(codexSessionHeader(rollout({ id: 'x' })), null);
    assert.equal(codexSessionHeader('{broken'), null);
});

test('chooseCodexSession: 폴더가 같고 프로세스 시작 이후인 대화가 정확히 하나일 때만 고른다', () => {
    const at = seconds => Date.parse('2026-10-14T01:00:00Z') + seconds * 1000;
    const item = (id, cwd, seconds) => ({ id, cwd, startedAt: at(seconds) });
    const processStartedAt = at(0);
    assert.deepEqual(chooseCodexSession([item('a', '/w', 10)], { cwd: '/w', processStartedAt }), { sessionId: 'a', cwd: '/w' });
    assert.deepEqual(chooseCodexSession([item('a', '/w', 10)], { cwd: '/w/', processStartedAt }), { sessionId: 'a', cwd: '/w' });
    assert.equal(chooseCodexSession([item('a', '/w', 10), item('b', '/w', 20)], { cwd: '/w', processStartedAt }), null);
    assert.equal(chooseCodexSession([item('a', '/other', 10)], { cwd: '/w', processStartedAt }), null);
    assert.equal(chooseCodexSession([item('a', '/w', -60)], { cwd: '/w', processStartedAt }), null);
    // 초 단위로만 아는 프로세스 시작 시각의 오차(3초) 안이면 같은 프로세스의 대화로 본다.
    assert.deepEqual(chooseCodexSession([item('a', '/w', -2)], { cwd: '/w', processStartedAt }), { sessionId: 'a', cwd: '/w' });
    assert.equal(chooseCodexSession([], { cwd: '/w', processStartedAt }), null);
});

test('listCodexSessions: 시작한 날과 다음 날 폴더의 rollout만 읽고 없는 폴더는 건너뛴다', () => {
    const started = new Date(2026, 9, 14, 23, 59, 0).getTime();
    const files = {
        '/h/.codex/sessions/2026/10/14': ['rollout-a.jsonl', 'notes.txt'],
        '/h/.codex/sessions/2026/10/15': ['rollout-b.jsonl'],
    };
    const fileSystem = {
        readdir: directory => { if (!files[directory]) throw new Error('ENOENT'); return files[directory]; },
        readFirstLine: file => rollout({ id: file.endsWith('a.jsonl') ? CODEX_ID : '01a0ae67-7d1b-7ba0-a75e-113eb195b381' }),
    };
    const found = listCodexSessions({ codexHome: '/h/.codex', processStartedAt: started, fileSystem });
    assert.deepEqual(found.map(item => item.id), [CODEX_ID, '01a0ae67-7d1b-7ba0-a75e-113eb195b381']);
});

test('elapsedSeconds: ps의 경과 시간을 초로 바꾼다', () => {
    assert.equal(elapsedSeconds('05:07'), 307);
    assert.equal(elapsedSeconds('  01:02:03\n'), 3723);
    assert.equal(elapsedSeconds('2-00:00:01'), 172801);
    assert.equal(elapsedSeconds('abc'), null);
    assert.equal(elapsedSeconds(''), null);
});

test('resumableAgent: claude는 sessions 기록으로, codex는 시작 시각과 폴더로 대화를 찾고, 그 밖은 null이다', () => {
    const deps = {
        homeDirectory: '/home/me',
        readFile: file => (file === '/home/me/.claude/sessions/42.json' ? JSON.stringify({ pid: 42, sessionId: CLAUDE_ID, kind: 'interactive', cwd: '/work/app' }) : (() => { throw new Error('ENOENT'); })()),
        fileSystem: { readdir: () => ['rollout-a.jsonl'], readFirstLine: () => rollout() },
    };
    assert.deepEqual(resumableAgent({ program: 'claude', pid: 42, env: {}, cwd: '/work/app', startedAt: null }, deps), { provider: 'claude', sessionId: CLAUDE_ID, cwd: '/work/app' });
    assert.equal(resumableAgent({ program: 'claude', pid: 43, env: {}, cwd: '/work/app', startedAt: null }, deps), null);
    const startedAt = Date.parse('2026-10-14T01:00:00Z');
    assert.deepEqual(resumableAgent({ program: 'codex', pid: 9, env: { HOME: '/home/me' }, cwd: '/work/app', startedAt }, deps), { provider: 'codex', sessionId: CODEX_ID, cwd: '/work/app' });
    // 시작 시각을 모르면 Codex 대화를 고르지 않는다.
    assert.equal(resumableAgent({ program: 'codex', pid: 9, env: {}, cwd: '/work/app', startedAt: null }, deps), null);
    assert.equal(resumableAgent({ program: 'bash', pid: 9, env: {}, cwd: '/work/app', startedAt }, deps), null);
});
