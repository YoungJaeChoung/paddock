// 터미널의 마지막 화면 출력과 실행 정보를 파일에 주기적으로 남기고, 앱이 터미널 프로세스째 사라진 뒤 그 터미널을 되살리는 데 쓰게 한다.
// 앱을 강제로 끝내는 갱신(Windows 설치 프로그램)·백엔드 비정상 종료는 끝날 때 저장할 틈이 없어, 출력이 올 때마다 미뤄서 저장해 둔다.
// 저장 위치는 `<설정 폴더>/terminal-history/<터미널 id>.json`이다. Theia·Electron을 불러오지 않아 단독으로 시험한다.
const fs = require('node:fs');
const nodePath = require('node:path');
const { TrailingDebounce } = require('./trailing-debounce');

class HistoryLimits {
    /** 출력이 멎은 뒤 저장하기까지 기다리는 시간(ms). */
    static QUIET_MS = 2000;
    /** 출력이 계속돼도 이 시간(ms) 안에는 한 번 저장한다. */
    static MAX_WAIT_MS = 10000;
    /** 화면 출력으로 남기는 마지막 부분의 최대 바이트. */
    static N_TAIL_BYTES = 256 * 1024;
    /** 이 기간(ms)이 지난 저장분은 되살릴 터미널이 아니라 버려진 기록이다. */
    static EXPIRE_MS = 14 * 24 * 60 * 60 * 1000;
    /** 저장하는 환경 변수. 계정 폴더와 WSL 표지만 둔다 — 로그인 키 같은 값은 파일에 남기지 않는다. */
    static ENVIRONMENT = Object.freeze(['CLAUDE_CONFIG_DIR', 'CODEX_HOME', 'WSLENV', 'PADDOCK_TERMINAL']);
}

/** OSC 7(셸이 현재 폴더를 알리는 제어 문자) 하나. 끝은 BEL 또는 ESC \. */
const OSC_CURRENT_FOLDER = /\x1b\]7;([^\x07\x1b]*)(?:\x07|\x1b\\)/g;

/**
 * OSC 7이 알린 값을 폴더 경로로 바꾼다. `file://호스트/경로` 형태는 경로 부분을 풀고, 그 밖의 값(Windows 셸이 보내는 `C:\…`)은 그대로 쓴다.
 *
 * Examples
 * --------
 * | 입력                          | 결과                |
 * |-------------------------------|---------------------|
 * | `file://host/home/me/a%20b`   | `/home/me/a b`      |
 * | `C:\work\app`                 | `C:\work\app`       |
 * | `file:///C:/work`             | `C:/work`           |
 */
function folderFromOsc7(
    value,
) {
    let result = value;
    if (/^file:\/\//i.test(value)) {
        try {
            result = decodeURIComponent(new URL(value).pathname);
            if (/^\/[a-z]:/i.test(result)) result = result.slice(1);
        } catch { /* 풀 수 없는 값은 그대로 둔다. */ }
    }
    return result;
}

/**
 * 터미널 출력의 마지막 N 바이트를 모으고, 셸이 마지막으로 알린 현재 폴더(OSC 7)를 기억한다.
 *
 * 읽을 때 한도를 넘는 앞부분은 글자 중간이 아닌 줄 처음부터 남긴다. 줄 경계가 가까이 없으면 글자 경계에서 자른다.
 *
 * Examples
 * --------
 * | 쓴 내용                           | 한도 | `text()`        |
 * |-----------------------------------|------|-----------------|
 * | `'ab'`, `'cd'`                    | 100  | `'abcd'`        |
 * | `'aaa\nbbb\nccc'`                 | 8    | `'bbb\nccc'`    |
 * | `'x'.repeat(10)` (줄 경계 없음)   | 4    | `'xxxx'`        |
 */
class OutputTail {
    constructor(
        maxBytes = HistoryLimits.N_TAIL_BYTES,
    ) {
        this.maxBytes = maxBytes;
        this.chunks = [];
        // chunks에 담긴 글자의 UTF-8 바이트 합. 한도의 두 배를 넘으면 앞부분을 덜어 메모리를 묶는다.
        this.n_bytes = 0;
        // 셸이 마지막으로 알린 현재 폴더. 알린 적 없으면 빈 문자열.
        this.reportedFolder = '';
        // OSC 7이 조각 경계에 걸릴 수 있어 앞 조각의 끝을 붙여 찾는다.
        this.carry = '';
    }

    append(
        text,
    ) {
        if (typeof text === 'string' && text) {
            const scanned = this.carry + text;
            for (const match of scanned.matchAll(OSC_CURRENT_FOLDER)) this.reportedFolder = folderFromOsc7(match[1]);
            this.carry = scanned.slice(-512);
            this.chunks.push(text);
            this.n_bytes += Buffer.byteLength(text);
            if (this.n_bytes > this.maxBytes * 2) {
                this.chunks = [this.text()];
                this.n_bytes = Buffer.byteLength(this.chunks[0]);
            }
        }
    }

    text() {
        const joined = this.chunks.join('');
        let result = joined;
        const bytes = Buffer.from(joined);
        if (bytes.length > this.maxBytes) {
            let start = bytes.length - this.maxBytes;
            // 글자 중간(이어지는 바이트)에서 시작하지 않게 글자 경계까지 민다.
            while (start < bytes.length && (bytes[start] & 0xc0) === 0x80) start += 1;
            result = bytes.subarray(start).toString('utf8');
            // 줄 처음부터 남긴다. 줄 경계가 앞쪽 1 KiB 안에 있을 때만 쓴다.
            const lineEnd = result.indexOf('\n');
            if (lineEnd >= 0 && lineEnd < 1024) result = result.slice(lineEnd + 1);
        }
        return result;
    }
}

/**
 * 저장할 환경 변수만 골라 낸다. 이름이 허용 목록에 없거나 값이 문자열이 아니면 버린다.
 *
 * Examples
 * --------
 * | 입력                                        | 결과                      |
 * |---------------------------------------------|---------------------------|
 * | `{ CODEX_HOME: '/c', ANTHROPIC_API_KEY: 'k' }` | `{ CODEX_HOME: '/c' }`  |
 * | `undefined`                                 | `{}`                      |
 */
function storableEnvironment(
    env,
) {
    const result = {};
    for (const name of HistoryLimits.ENVIRONMENT) {
        if (env && typeof env[name] === 'string') result[name] = env[name];
    }
    return result;
}

/**
 * 터미널 프로세스를 추적해 화면 출력과 실행 정보를 주기적으로 파일에 남긴다.
 *
 * 추적 대상 프로세스(`TerminalProcess`)는 `id`·`options`(`{ command, args, options: { cwd, env } }`)·`ringBuffer.addWriteListener`·`onExit`를 가져야 한다.
 * 프로세스가 사용자의 닫기·`exit`로 끝나면 저장분을 지운다. 앱이 끝내는 중(`beginStop` 뒤)에 끝나는 프로세스의 저장분은 남긴다 — 다시 열 때 되살린다.
 *
 * `inspect(process)`는 저장 직전에 부르는 `{ cwd, agent, foreground } | null`(프로세스가 지금 있는 폴더, 이어 열 수 있는 에이전트 대화, 앞쪽에서 실행 중인 에이전트 이름)이고 `Promise`여도 된다.
 * `isEnabled()`가 false면(사용자가 되살리기를 껐을 때) 저장하지 않고 이미 있는 저장분도 지운다.
 * `timers`·`now`는 시험에서 시간을 대신하려고 바꿔 끼운다.
 */
class TerminalHistory {
    constructor(
        {
            directory,
            inspect = () => null,
            isEnabled = () => true,
            timers = { setTimeout: (callback, delay) => setTimeout(callback, delay), clearTimeout: id => clearTimeout(id) },
            now = Date.now,
            onError = () => {},
        },
    ) {
        this.directory = directory;
        this.inspect = inspect;
        this.isEnabled = isEnabled;
        this.timers = timers;
        this.now = now;
        this.onError = onError;
        // 추적 중인 터미널 id → { process, tail, debounce, info, disposables }. 출력이 올 때마다 저장을 미룬다: 멎은 뒤 QUIET_MS, 계속 와도 MAX_WAIT_MS 안에 저장한다.
        this.entries = new Map();
        this.isStopping = false;
    }

    /** 저장 파일 경로. id는 숫자만 받는다. */
    fileOf(
        id,
    ) {
        if (!/^\d+$/.test(String(id))) throw new Error('The terminal id is invalid.');
        return nodePath.join(this.directory, `${id}.json`);
    }

    /** 터미널 프로세스를 추적하기 시작한다. 가짜 터미널(`isPseudo`)은 되살릴 대상이 아니라 추적하지 않는다. */
    track(
        terminalProcess,
    ) {
        if (!terminalProcess.options?.isPseudo && !this.entries.has(terminalProcess.id)) {
            const entry = { process: terminalProcess, tail: new OutputTail(), debounce: null, info: null, disposables: [] };
            entry.debounce = new TrailingDebounce({
                quietMs: HistoryLimits.QUIET_MS, maxWaitMs: HistoryLimits.MAX_WAIT_MS, run: () => void this.save(entry), timers: this.timers, now: this.now,
            });
            this.entries.set(terminalProcess.id, entry);
            entry.disposables.push(terminalProcess.ringBuffer.addWriteListener(text => {
                entry.tail.append(text);
                entry.debounce.touch();
            }));
            entry.disposables.push(terminalProcess.onExit(() => this.untrack(entry)));
        }
    }

    /** 지금까지의 출력과 실행 정보를 파일로 쓴다. 실패는 `onError`로 알리고 터미널에는 영향을 주지 않는다. */
    async save(
        entry,
    ) {
        try {
            let info = null;
            try {
                info = await this.inspect(entry.process);
            } catch { /* 폴더·대화를 못 읽으면 지난 값을 쓴다. */ }
            if (info) entry.info = info;
            if (this.entries.get(entry.process.id) === entry) this.write(entry);
        } catch (error) {
            this.onError(error);
        }
    }

    /** 추적 중인 모든 터미널을 지금 바로(알고 있는 정보로) 저장한다. 앱을 끝내기 직전 마지막 출력을 남긴다. */
    flushAll() {
        for (const entry of this.entries.values()) {
            entry.debounce.cancel();
            try {
                this.write(entry);
            } catch (error) {
                this.onError(error);
            }
        }
    }

    /** 앱이 터미널을 끝내려는 중임을 알린다. 이후 끝나는 터미널의 저장분은 지우지 않고 남기며, 마지막 출력을 저장한다. */
    beginStop() {
        if (!this.isStopping) {
            this.isStopping = true;
            this.flushAll();
        }
    }

    write(
        entry,
    ) {
        if (this.isEnabled()) {
            this.writeRecord(entry);
        } else {
            this.remove(entry.process.id);
        }
    }

    writeRecord(
        entry,
    ) {
        const options = entry.process.options || {};
        const launcher = {
            command: options.command,
            args: Array.isArray(options.args) ? options.args : [],
            cwd: options.options?.cwd,
            env: storableEnvironment(options.options?.env),
        };
        // 현재 폴더는 프로세스를 직접 읽은 값, 셸이 알린 값(OSC 7), 처음 폴더 순으로 쓴다.
        const record = {
            version: 1,
            savedAt: this.now(),
            launcher,
            cwdNow: entry.info?.cwd || entry.tail.reportedFolder || launcher.cwd || '',
            agent: entry.info?.agent || null,
            foreground: entry.info?.foreground || null,
            output: entry.tail.text(),
        };
        // 화면 출력에는 비밀이 찍혔을 수 있다. 폴더는 0700, 파일은 0600으로 둔다(Windows에서는 무시된다).
        fs.mkdirSync(this.directory, { recursive: true, mode: 0o700 });
        fs.chmodSync(this.directory, 0o700);
        const file = this.fileOf(entry.process.id);
        // 쓰는 도중 앱이 끝나도 반쪽 파일이 남지 않게 임시 파일에 쓴 뒤 옮긴다.
        const temporary = `${file}.${process.pid}.tmp`;
        fs.writeFileSync(temporary, JSON.stringify(record), { mode: 0o600 });
        fs.renameSync(temporary, file);
    }

    /** 프로세스가 끝났다. 추적을 멈추고, 앱이 끝내는 중이 아니면(사용자가 닫았거나 `exit`) 저장분을 지운다. */
    untrack(
        entry,
    ) {
        const id = entry.process.id;
        if (this.entries.get(id) === entry) {
            this.entries.delete(id);
            entry.debounce.cancel();
            for (const disposable of entry.disposables) disposable?.dispose?.();
            if (!this.isStopping) this.remove(id);
        }
    }

    /** 저장된 터미널 기록을 읽는다. 없거나 깨졌으면 null이다. */
    read(
        id,
    ) {
        let record = null;
        try {
            const parsed = JSON.parse(fs.readFileSync(this.fileOf(id), 'utf8'));
            if (parsed && parsed.version === 1 && typeof parsed.output === 'string') record = parsed;
        } catch { /* 기록이 없거나 깨졌으면 되살릴 것이 없다. */ }
        return record;
    }

    /** 저장된 터미널 기록을 지운다. 없으면 아무 일도 하지 않는다. */
    remove(
        id,
    ) {
        try {
            fs.rmSync(this.fileOf(id), { force: true });
        } catch (error) {
            this.onError(error);
        }
    }

    /** 오래된 기록을 지운다. 앱 시작 때 한 번 부른다. 지운 개수를 돌려준다. */
    prune() {
        let n_removed = 0;
        let names = [];
        try {
            names = fs.readdirSync(this.directory);
        } catch { /* 폴더가 없으면 지울 것도 없다. */ }
        for (const name of names) {
            const file = nodePath.join(this.directory, name);
            try {
                if (this.now() - fs.statSync(file).mtimeMs > HistoryLimits.EXPIRE_MS) {
                    fs.rmSync(file, { force: true });
                    n_removed += 1;
                }
            } catch { /* 지울 수 없는 파일은 다음 시작에 다시 본다. */ }
        }
        return n_removed;
    }
}

module.exports = { HistoryLimits, OutputTail, TerminalHistory, folderFromOsc7, storableEnvironment };
