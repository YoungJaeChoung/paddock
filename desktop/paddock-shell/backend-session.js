// 앱을 닫아도 남는 백엔드(터미널 프로세스를 쥔 프로세스)의 접속 정보를 읽고 쓰며, 살아 있는지 확인하고 끝낸다.
// Electron 메인 프로세스가 앱 시작 때 남은 백엔드에 다시 붙고, 종료 때 백엔드를 남길지 끝낼지 정하는 데 쓴다.
const childProcess = require('node:child_process');
const http = require('node:http');
const fs = require('node:fs');
const net = require('node:net');
const path = require('node:path');

/** 접속 정보를 두는 파일 이름. 설정 폴더 안에 둔다. */
const STATE_FILE = 'backend.json';

/**
 * 이번 종료에 한해 남은 터미널을 어떻게 할지 앱 화면이 정해 남기는 표지 파일 이름. 내용은 `선택:요청 시각(ms)`이다(예: `stop:1700000000000`).
 * 선택은 `keep`(계속 실행)·`stop`(모두 끝냄)이다. 마지막 창이 닫히기 전에 창 안에서 물은 답이나
 * "Quit and Stop All Terminals" 명령이 적고, 메인 프로세스가 종료를 확정할 때 읽고 지운다.
 */
const QUIT_CHOICE_FILE = 'backend-quit-choice';

/** 종료 선택 표지의 값. */
const QUIT_CHOICE = Object.freeze({ KEEP: 'keep', STOP: 'stop' });

/** 종료 선택 표지가 유효한 시간(ms). 저장 확인 때문에 창 닫기가 늦어져도 이 안에는 받아들이고, 넘으면 낡은 표지로 버린다. */
const QUIT_CHOICE_TTL_MS = 5 * 60 * 1000;

/** 시작·종료 결정을 한 번에 한 앱만 하도록 잡는 잠금 파일 이름. 내용은 잠금을 쥔 프로세스의 PID다. */
const LOCK_FILE = 'backend.lock';

/** 백엔드가 끝난 것을 기다리는 최대 시간(ms). */
const STOP_TIMEOUT_MS = 8000;

/**
 * 남은 백엔드의 접속 정보를 읽는다.
 *
 * 앱이 새로 띄운 백엔드가 아니라 이전 실행이 남긴 것을 다시 쓰기 위한 정보다.
 * 파일이 없거나 형식이 맞지 않으면 `null`이다.
 *
 * Examples
 * --------
 * | 파일 내용                                         | 결과                                            |
 * |---------------------------------------------------|-------------------------------------------------|
 * | `{"pid":7,"port":9,"token":"t","fingerprint":"f"}` | `{ pid: 7, port: 9, token: 't', fingerprint: 'f' }` |
 * | 없음 · 깨진 JSON · 필드 누락                      | `null`                                          |
 */
function readState(
    directory,
) {
    let state = null;
    try {
        const parsed = JSON.parse(fs.readFileSync(path.join(directory, STATE_FILE), 'utf8'));
        if (Number.isInteger(parsed.pid) && parsed.pid > 0 && Number.isInteger(parsed.port) && parsed.port > 0 && parsed.port < 65536 && typeof parsed.token === 'string') {
            state = { pid: parsed.pid, port: parsed.port, token: parsed.token, fingerprint: String(parsed.fingerprint || '') };
        }
    } catch { /* 파일이 없거나 깨졌다. 남은 백엔드가 없는 것으로 본다. */ }
    return state;
}

/** 접속 정보를 쓴다. 토큰이 들어 있어 소유자만 읽도록 권한을 0600으로 둔다. */
function writeState(
    directory,
    state,
) {
    const file = path.join(directory, STATE_FILE);
    fs.writeFileSync(file, JSON.stringify(state), { mode: 0o600 });
    // 이미 있던 파일의 권한은 writeFileSync의 mode가 바꾸지 않는다.
    fs.chmodSync(file, 0o600);
}

/** 종료 선택 표지에 적을 내용. `at`은 요청 시각(ms)이다. */
function formatQuitChoice(
    choice,
    at,
) {
    return `${choice}:${at}`;
}

/**
 * 종료 선택 표지를 읽고 지운다. 표지는 한 번만 쓰이므로 읽으면 항상 지운다. `now`는 현재 시각(ms)이다.
 *
 * Examples
 * --------
 * | 표지 내용(now = 1000)          | 결과     |
 * | ------------------------------ | -------- |
 * | `stop:900`                     | `'stop'` |
 * | `keep:900`                     | `'keep'` |
 * | 없음 · `stop:-400000`(낡음) · 깨짐 | `null`   |
 */
function takeQuitChoice(
    directory,
    now,
) {
    const file = path.join(directory, QUIT_CHOICE_FILE);
    let choice = null;
    try {
        const [value, at] = fs.readFileSync(file, 'utf8').trim().split(':');
        const requestedAt = Number(at);
        const isFresh = Number.isFinite(requestedAt) && now - requestedAt >= 0 && now - requestedAt <= QUIT_CHOICE_TTL_MS;
        if (isFresh && Object.values(QUIT_CHOICE).includes(value)) choice = value;
    } catch { /* 표지가 없다. */ }
    fs.rmSync(file, { force: true });
    return choice;
}

/** 접속 정보를 지운다. 이미 없으면 아무것도 하지 않는다. */
function clearState(
    directory,
) {
    fs.rmSync(path.join(directory, STATE_FILE), { force: true });
}

/** 프로세스가 살아 있는지. 권한이 없어 신호를 못 보내는 경우(EPERM)도 살아 있는 것이다. */
function isAlive(
    pid,
) {
    let alive = false;
    try {
        process.kill(pid, 0);
        alive = true;
    } catch (error) {
        alive = error.code === 'EPERM';
    }
    return alive;
}

/** 로컬 포트에 연결이 되는지. */
function isListening(
    port,
) {
    return new Promise((resolve) => {
        const socket = net.connect({ port, host: '127.0.0.1' });
        socket.once('connect', () => { socket.destroy(); resolve(true); });
        socket.once('error', () => resolve(false));
    });
}

/** 비어 있는 로컬 포트 하나를 얻는다. */
function freePort() {
    return new Promise((resolve, reject) => {
        const server = net.createServer();
        server.once('error', reject);
        server.listen(0, '127.0.0.1', () => {
            const { port } = server.address();
            server.close(() => resolve(port));
        });
    });
}

/**
 * 백엔드 실행 파일이 바뀌었는지 알아보는 지문. 앱 버전과 진입 파일의 수정 시각을 묶는다.
 * 버전을 올리지 않은 개발 빌드도 백엔드 코드가 달라지면 다른 지문이 된다.
 */
function fingerprintOf(
    version,
    backendMainPath,
) {
    let modified = 0;
    try {
        modified = Math.floor(fs.statSync(backendMainPath).mtimeMs);
    } catch { /* 파일을 못 읽으면 수정 시각 0으로 둔다. */ }
    return `${version}:${modified}`;
}

/** 백엔드에 넘겨 새 터미널이 받게 할 환경 변수 이름. 앱을 다시 열 때 바뀌었을 수 있는 것만 둔다. */
const SHARED_ENVIRONMENT_NAMES = [
    'PATH', 'Path', 'PATHEXT', 'WSLENV', 'SSH_AUTH_SOCK',
    'CLAUDE_CONFIG_DIR', 'CODEX_HOME',
    'HTTP_PROXY', 'HTTPS_PROXY', 'NO_PROXY', 'http_proxy', 'https_proxy', 'no_proxy',
];

/**
 * 환경 변수 객체에서 백엔드에 공유할 변수만 골라 문자열 값으로 돌려준다. 이름이 허용 목록에 없거나 값이 문자열이 아니면 버린다.
 *
 * Examples
 * --------
 * | 입력                                  | 결과               |
 * |---------------------------------------|--------------------|
 * | `{ PATH: '/bin', SECRET: 'x' }`       | `{ PATH: '/bin' }` |
 * | `{ PATH: 5 }` · `null`                | `{}`               |
 */
function sharedEnvironment(
    env,
) {
    const shared = {};
    if (env && typeof env === 'object') {
        for (const name of SHARED_ENVIRONMENT_NAMES) {
            if (typeof env[name] === 'string') shared[name] = env[name];
        }
    }
    return shared;
}

/**
 * 백엔드의 HTTP 경로를 부른다. 토큰은 Electron 보안 토큰이며 쿠키로 보낸다.
 * 연결하지 못하거나 응답이 늦으면 `null`, 아니면 `{ status, body }`(body는 JSON이면 객체, 아니면 문자열)다.
 */
function callBackend(
    port,
    token,
    method,
    route,
    body,
) {
    return new Promise((resolve) => {
        const payload = body === undefined ? null : JSON.stringify(body);
        const request = http.request({
            host: '127.0.0.1',
            port,
            path: route,
            method,
            timeout: 5000,
            headers: {
                cookie: `THEIA_ELECTRON_TOKEN=${encodeURIComponent(JSON.stringify({ value: token }))}`,
                ...(payload ? { 'content-type': 'application/json', 'content-length': Buffer.byteLength(payload) } : {}),
            },
        }, (response) => {
            let text = '';
            response.on('data', (chunk) => { text += chunk; });
            response.on('end', () => {
                let parsed = text;
                try { parsed = JSON.parse(text); } catch { /* 본문이 JSON이 아니면 문자열로 둔다. */ }
                resolve({ status: response.statusCode, body: parsed });
            });
        });
        request.on('error', () => resolve(null));
        request.on('timeout', () => { request.destroy(); resolve(null); });
        if (payload) request.write(payload);
        request.end();
    });
}

/** 백엔드에서 열려 있는 터미널 수. 읽지 못하면(연결 실패·옛 버전의 백엔드) `null`이다. */
async function countTerminals(
    port,
    token,
) {
    const reply = await callBackend(port, token, 'GET', '/paddock/session');
    let n_terminals = null;
    if (reply && reply.status === 200 && reply.body && Number.isInteger(reply.body.terminals)) n_terminals = reply.body.terminals;
    return n_terminals;
}

/** 지금 앱의 환경 변수를 백엔드에 넘긴다. 이후 새로 여는 터미널이 받는다. 실패해도 앱 동작에는 영향이 없다. */
async function shareEnvironment(
    port,
    token,
    env,
) {
    await callBackend(port, token, 'POST', '/paddock/session/environment', { env: sharedEnvironment(env) });
}

/**
 * 백엔드를 정상 절차로 끝내고 사라질 때까지 기다린다. 정상 절차가 통하지 않으면(옛 버전의 백엔드·응답 없음) 신호로 끝낸다.
 * 백엔드는 정상 종료 때 자기 터미널 프로세스도 함께 정리한다. 제한 시간 안에 끝나지 않으면 강제 종료한다.
 */
async function stop(
    backend,
) {
    const { pid, port, token } = backend;
    if (isAlive(pid)) {
        const reply = port && token ? await callBackend(port, token, 'POST', '/paddock/session/stop') : null;
        if (!reply || reply.status !== 202) {
            if (process.platform === 'win32') {
                childProcess.spawnSync('taskkill.exe', ['/pid', String(pid), '/t', '/f'], { windowsHide: true });
            } else {
                signalGroup(pid, 'SIGTERM');
            }
        }
        const n_polls = STOP_TIMEOUT_MS / 100;
        for (let n_waits = 0; isAlive(pid) && n_waits < n_polls; n_waits += 1) {
            await new Promise((resolve) => setTimeout(resolve, 100));
        }
        if (isAlive(pid)) {
            if (process.platform === 'win32') childProcess.spawnSync('taskkill.exe', ['/pid', String(pid), '/t', '/f'], { windowsHide: true });
            else signalGroup(pid, 'SIGKILL');
        }
    }
}

/**
 * 시작·종료 결정을 한 번에 한 앱만 하도록 설정 폴더의 잠금 파일을 잡는다. 잡았으면 해제 함수를, 제한 시간 안에 못 잡으면 `null`이다.
 * 잠금을 쥔 프로세스가 이미 없으면 낡은 잠금으로 보고 가져온다.
 */
async function acquireLock(
    directory,
    timeoutMs = 30000,
) {
    const file = path.join(directory, LOCK_FILE);
    let release = null;
    const deadline = timeoutMs / 100;
    for (let n_tries = 0; release === null && n_tries <= deadline; n_tries += 1) {
        try {
            fs.writeFileSync(file, String(process.pid), { flag: 'wx' });
            release = () => fs.rmSync(file, { force: true });
        } catch (error) {
            if (error.code !== 'EEXIST') throw error;
            let owner = 0;
            try { owner = Number(fs.readFileSync(file, 'utf8')); } catch { /* 그 사이에 풀렸다. 다시 시도한다. */ }
            if (owner !== process.pid && !(owner > 0 && isAlive(owner))) fs.rmSync(file, { force: true });
            else await new Promise((resolve) => setTimeout(resolve, 100));
        }
    }
    return release;
}

/**
 * 백엔드와 그 도우미 프로세스에 신호를 보낸다. Unix에서 분리해 띄운 백엔드는 자기 그룹의 대표라 그룹 전체에 보낸다.
 * 그룹이 아니면(대표가 아니면) 그 프로세스에만 보낸다.
 */
function signalGroup(
    pid,
    signal,
) {
    try {
        process.kill(-pid, signal);
    } catch {
        try { process.kill(pid, signal); } catch { /* 그 사이에 끝났다. */ }
    }
}

module.exports = { STATE_FILE, QUIT_CHOICE_FILE, QUIT_CHOICE, formatQuitChoice, takeQuitChoice, readState, writeState, clearState, isAlive, isListening, freePort, fingerprintOf, sharedEnvironment, callBackend, countTerminals, shareEnvironment, stop, acquireLock };
