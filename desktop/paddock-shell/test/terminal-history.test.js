const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { TerminalHistory, OutputTail, HistoryLimits, folderFromOsc7, storableEnvironment } = require('../terminal-history');

/** 가짜 시계: 예약한 함수를 시각이 되면 실행한다. */
function fakeClock() {
    const clock = {
        time: 1000,
        jobs: new Map(),
        n_next: 1,
        setTimeout: (callback, delay) => { const id = clock.n_next; clock.n_next += 1; clock.jobs.set(id, { at: clock.time + delay, callback }); return id; },
        clearTimeout: id => { clock.jobs.delete(id); },
        now: () => clock.time,
        async advance(ms) {
            clock.time += ms;
            for (const [id, job] of [...clock.jobs]) {
                if (job.at <= clock.time) { clock.jobs.delete(id); job.callback(); }
            }
            await new Promise(resolve => setImmediate(resolve));
        },
    };
    return clock;
}

/** 터미널 프로세스를 흉내 낸다: 쓰기 알림 함수와 종료 알림 함수를 직접 부른다. */
function fakeProcess(id, options = {}) {
    const writers = new Set();
    const exits = new Set();
    return {
        id,
        options: { command: '/bin/bash', args: ['-l'], options: { cwd: '/work/app', env: { PATH: '/bin', CODEX_HOME: '/c', ANTHROPIC_API_KEY: 'secret' } }, ...options },
        ringBuffer: { addWriteListener: listener => { writers.add(listener); return { dispose: () => writers.delete(listener) }; } },
        onExit: listener => { exits.add(listener); return { dispose: () => exits.delete(listener) }; },
        write: text => writers.forEach(listener => listener(text)),
        exit: () => exits.forEach(listener => listener({ code: 0 })),
        n_writers: () => writers.size,
    };
}

function setup(extra = {}) {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-history-'));
    const clock = fakeClock();
    const history = new TerminalHistory({ directory, timers: clock, now: clock.now, ...extra });
    return { directory, clock, history };
}

test('OutputTail: 한도를 넘으면 줄 처음부터 남기고, 글자 중간에서 자르지 않는다', () => {
    const tail = new OutputTail(8);
    tail.append('aaa\nbbb\nccc');
    assert.equal(tail.text(), 'bbb\nccc');
    const wide = new OutputTail(4);
    wide.append('가나다');
    // 9바이트 중 마지막 4바이트는 '다'(3바이트) 앞의 이어지는 바이트를 건너뛴 '다'다.
    assert.equal(wide.text(), '다');
    const plain = new OutputTail(4);
    plain.append('x'.repeat(10));
    assert.equal(plain.text(), 'xxxx');
});

test('OutputTail: 한도의 두 배를 넘게 쌓아도 마지막 한도만큼만 유지한다', () => {
    const tail = new OutputTail(100);
    for (let i = 0; i < 500; i += 1) tail.append(`line ${i}\n`);
    const text = tail.text();
    assert.ok(Buffer.byteLength(text) <= 100);
    assert.ok(text.endsWith('line 499\n'));
    assert.ok(tail.chunks.length < 100);
});

test('OSC 7: 셸이 알린 마지막 폴더를 기억하고 조각 경계에 걸려도 읽는다', () => {
    const tail = new OutputTail();
    tail.append('x\x1b]7;C:\\work\\a\x07 prompt');
    assert.equal(tail.reportedFolder, 'C:\\work\\a');
    tail.append('\x1b]7;file://host/home/me/b%20c');
    assert.equal(tail.reportedFolder, 'C:\\work\\a');
    tail.append('\x07$ ');
    assert.equal(tail.reportedFolder, '/home/me/b c');
    assert.equal(folderFromOsc7('file:///C:/work'), 'C:/work');
    assert.equal(folderFromOsc7('/plain'), '/plain');
});

test('storableEnvironment: 허용한 이름만 문자열로 남긴다', () => {
    assert.deepEqual(storableEnvironment({ CODEX_HOME: '/c', ANTHROPIC_API_KEY: 'k', WSLENV: 5, PADDOCK_TERMINAL: 't' }), { CODEX_HOME: '/c', PADDOCK_TERMINAL: 't' });
    assert.deepEqual(storableEnvironment(undefined), {});
});

test('출력이 멎은 뒤 2초에 저장하고, 계속 와도 10초 안에 저장한다', async () => {
    const { directory, clock, history } = setup();
    const terminal = fakeProcess(7);
    history.track(terminal);
    terminal.write('hello');
    await clock.advance(1500);
    assert.equal(fs.existsSync(path.join(directory, '7.json')), false);
    await clock.advance(600);
    assert.equal(history.read(7).output, 'hello');

    // 1초마다 출력이 계속되면 멎지 않아도 첫 미룸으로부터 10초 안에 한 번 저장한다.
    fs.rmSync(path.join(directory, '7.json'));
    for (let i = 0; i < 10; i += 1) { terminal.write(`n${i}`); await clock.advance(1000); }
    assert.equal(history.read(7) !== null, true);
    assert.match(history.read(7).output, /n0n1/);
});

test('기록에는 셸·인자·처음 폴더·허용한 환경 변수·현재 폴더가 들어간다', async () => {
    const { clock, history } = setup({ inspect: () => ({ cwd: '/work/app/sub', agent: { provider: 'claude', sessionId: 's', cwd: '/work/app/sub' } }) });
    const terminal = fakeProcess(8);
    history.track(terminal);
    terminal.write('x');
    await clock.advance(2100);
    const record = history.read(8);
    assert.deepEqual(record.launcher, { command: '/bin/bash', args: ['-l'], cwd: '/work/app', env: { CODEX_HOME: '/c' } });
    assert.equal(record.cwdNow, '/work/app/sub');
    assert.deepEqual(record.agent, { provider: 'claude', sessionId: 's', cwd: '/work/app/sub' });
    assert.equal(JSON.stringify(record).includes('secret'), false);
});

test('현재 폴더는 읽은 값, 셸이 알린 값(OSC 7), 처음 폴더 순으로 쓴다', async () => {
    const { clock, history } = setup();
    const terminal = fakeProcess(9);
    history.track(terminal);
    terminal.write('a');
    await clock.advance(2100);
    assert.equal(history.read(9).cwdNow, '/work/app');
    terminal.write('\x1b]7;/work/other\x07');
    await clock.advance(2100);
    assert.equal(history.read(9).cwdNow, '/work/other');
});

test('읽기가 실패하면 지난 정보를 쓰고 터미널은 계속 저장한다', async () => {
    let n_calls = 0;
    const { clock, history } = setup({ inspect: () => { n_calls += 1; if (n_calls === 2) throw new Error('boom'); return { cwd: '/ok', agent: null }; } });
    const terminal = fakeProcess(10);
    history.track(terminal);
    terminal.write('a');
    await clock.advance(2100);
    terminal.write('b');
    await clock.advance(2100);
    assert.equal(history.read(10).cwdNow, '/ok');
    assert.equal(history.read(10).output, 'ab');
});

test('사용자가 터미널을 끝내면 기록을 지우고, 앱이 끝내는 중이면 남긴다', async () => {
    const { clock, history } = setup();
    const closed = fakeProcess(11);
    const interrupted = fakeProcess(12);
    history.track(closed);
    history.track(interrupted);
    closed.write('a');
    interrupted.write('b');
    await clock.advance(2100);
    assert.notEqual(history.read(11), null);
    closed.exit();
    assert.equal(history.read(11), null);
    assert.equal(closed.n_writers(), 0);

    // 저장 전에 멈추는 신호가 와도 마지막 출력까지 저장한다.
    interrupted.write('c');
    history.beginStop();
    interrupted.exit();
    assert.equal(history.read(12).output, 'bc');
});

test('되살리기를 끄면 저장하지 않고 이미 있는 기록도 지운다', async () => {
    let enabled = true;
    const { clock, history } = setup({ isEnabled: () => enabled });
    const terminal = fakeProcess(13);
    history.track(terminal);
    terminal.write('a');
    await clock.advance(2100);
    assert.notEqual(history.read(13), null);
    enabled = false;
    terminal.write('b');
    await clock.advance(2100);
    assert.equal(history.read(13), null);
});

test('가짜 터미널은 추적하지 않고, 같은 프로세스를 두 번 추적하지 않는다', () => {
    const { history } = setup();
    const pseudo = fakeProcess(14, { isPseudo: true });
    history.track(pseudo);
    assert.equal(pseudo.n_writers(), 0);
    const terminal = fakeProcess(15);
    history.track(terminal);
    history.track(terminal);
    assert.equal(terminal.n_writers(), 1);
});

test('read는 없거나 깨진 기록에 null이고, id는 숫자만 받는다', () => {
    const { directory, history } = setup();
    assert.equal(history.read(99), null);
    fs.writeFileSync(path.join(directory, '98.json'), '{broken');
    assert.equal(history.read(98), null);
    assert.equal(history.read('../x'), null);
    assert.throws(() => history.fileOf('../x'));
});

test('prune: 14일이 지난 기록만 지운다', () => {
    const { directory, history } = setup({ now: () => Date.now() });
    fs.writeFileSync(path.join(directory, '1.json'), '{}');
    fs.writeFileSync(path.join(directory, '2.json'), '{}');
    const old = (Date.now() - HistoryLimits.EXPIRE_MS - 1000) / 1000;
    fs.utimesSync(path.join(directory, '1.json'), old, old);
    assert.equal(history.prune(), 1);
    assert.deepEqual(fs.readdirSync(directory), ['2.json']);
});

test('저장은 반쪽 파일을 남기지 않는다(임시 파일은 옮긴 뒤 없다)', async () => {
    const { directory, clock, history } = setup();
    const terminal = fakeProcess(16);
    history.track(terminal);
    terminal.write('a');
    await clock.advance(2100);
    assert.deepEqual(fs.readdirSync(directory), ['16.json']);
});
