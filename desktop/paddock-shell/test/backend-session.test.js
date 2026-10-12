const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const BackendSession = require('../backend-session');

function temporaryDirectory() {
    return fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-backend-'));
}

test('상태를 쓰고 읽으면 같은 값이고 파일 권한은 소유자 전용이다', () => {
    const directory = temporaryDirectory();
    BackendSession.writeState(directory, { pid: 7, port: 9, token: 't', fingerprint: 'f' });
    assert.deepEqual(BackendSession.readState(directory), { pid: 7, port: 9, token: 't', fingerprint: 'f' });
    if (process.platform !== 'win32') {
        assert.equal(fs.statSync(path.join(directory, BackendSession.STATE_FILE)).mode & 0o777, 0o600);
    }
});

test('없거나 깨졌거나 필드가 빠진 상태는 null이다', () => {
    const directory = temporaryDirectory();
    assert.equal(BackendSession.readState(directory), null);
    fs.writeFileSync(path.join(directory, BackendSession.STATE_FILE), '{broken');
    assert.equal(BackendSession.readState(directory), null);
    fs.writeFileSync(path.join(directory, BackendSession.STATE_FILE), JSON.stringify({ pid: 1 }));
    assert.equal(BackendSession.readState(directory), null);
});

test('clearState는 파일이 없어도 실패하지 않는다', () => {
    const directory = temporaryDirectory();
    BackendSession.clearState(directory);
    BackendSession.writeState(directory, { pid: 7, port: 9, token: 't', fingerprint: 'f' });
    BackendSession.clearState(directory);
    assert.equal(BackendSession.readState(directory), null);
});

test('fingerprintOf는 파일이 바뀌면 달라지고 없으면 0이다', () => {
    const directory = temporaryDirectory();
    const file = path.join(directory, 'main.js');
    assert.equal(BackendSession.fingerprintOf('1.0.0', file), '1.0.0:0');
    fs.writeFileSync(file, 'a');
    fs.utimesSync(file, 1000, 1000);
    const first = BackendSession.fingerprintOf('1.0.0', file);
    fs.utimesSync(file, 2000, 2000);
    assert.notEqual(BackendSession.fingerprintOf('1.0.0', file), first);
});

test('stop은 살아 있는 프로세스를 끝내고 없는 프로세스에는 아무것도 하지 않는다', async () => {
    const child = require('node:child_process').spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], { stdio: 'ignore' });
    assert.equal(BackendSession.isAlive(child.pid), true);
    await BackendSession.stop({ pid: child.pid });
    assert.equal(BackendSession.isAlive(child.pid), false);
    await BackendSession.stop({ pid: child.pid });
});

test('sharedEnvironment는 허용한 이름의 문자열 값만 남긴다', () => {
    assert.deepEqual(BackendSession.sharedEnvironment({ PATH: '/bin', SECRET: 'x', CODEX_HOME: 5 }), { PATH: '/bin' });
    assert.deepEqual(BackendSession.sharedEnvironment(null), {});
});

test('acquireLock은 한 번에 한 곳만 잡고 낡은 잠금은 가져온다', async () => {
    const directory = temporaryDirectory();
    const first = await BackendSession.acquireLock(directory, 300);
    assert.equal(typeof first, 'function');
    // 같은 프로세스가 쥔 잠금은 해제 전까지 다시 잡히지 않는다.
    assert.equal(await BackendSession.acquireLock(directory, 300), null);
    first();
    const second = await BackendSession.acquireLock(directory, 300);
    assert.equal(typeof second, 'function');
    second();
    // 이미 없는 프로세스의 잠금은 낡은 것이다.
    fs.writeFileSync(path.join(directory, 'backend.lock'), '2147483646');
    const third = await BackendSession.acquireLock(directory, 300);
    assert.equal(typeof third, 'function');
    third();
});

test('takeStopOnce는 유효한 표지를 한 번만 받아들이고 항상 지운다', () => {
    const directory = temporaryDirectory();
    const file = path.join(directory, BackendSession.STOP_ONCE_FILE);
    assert.equal(BackendSession.takeStopOnce(directory, 1000), false);
    fs.writeFileSync(file, '900');
    assert.equal(BackendSession.takeStopOnce(directory, 1000), true);
    assert.equal(fs.existsSync(file), false);
    fs.writeFileSync(file, String(1000 - 6 * 60 * 1000));
    assert.equal(BackendSession.takeStopOnce(directory, 1000), false);
    assert.equal(fs.existsSync(file), false);
    fs.writeFileSync(file, 'abc');
    assert.equal(BackendSession.takeStopOnce(directory, 1000), false);
});
