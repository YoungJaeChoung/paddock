const test = require('node:test');
const assert = require('node:assert/strict');
const wsl = require('../wsl-terminals');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const childProcess = require('node:child_process');

test('C-wsl.1: wsl.exe 셸만 WSL 터미널로 본다', () => {
    assert.equal(wsl.isWslShell('C:\\Windows\\System32\\wsl.exe'), true);
    assert.equal(wsl.isWslShell('wsl'), true);
    assert.equal(wsl.isWslShell('C:\\Program Files\\Git\\bin\\bash.exe'), false);
    assert.equal(wsl.isWslShell(undefined), false);
});

test('C-wsl.2: 스크립트 출력에서 터미널별 현재 폴더와 명령줄을 읽고 깨진 줄은 버린다', () => {
    const output = 't1\t\\\\wsl.localhost\\Ubuntu\\home\\me\\app\tnode\x1f/usr/bin/claude\x1f\r\nt2\t\t-bash\x1f\nbroken line\n';
    assert.deepEqual(wsl.parse(output), {
        t1: { cwd: '\\\\wsl.localhost\\Ubuntu\\home\\me\\app', argv: ['node', '/usr/bin/claude'], env: {}, distribution: '', sessionRecord: '', pid: '' },
        t2: { cwd: '', argv: ['-bash'], env: {}, distribution: '', sessionRecord: '', pid: '' },
    });
    assert.deepEqual(wsl.parse(''), {});
});

test('C-wsl.3: 넷째 칸의 계정 환경은 CODEX_HOME·CLAUDE_CONFIG_DIR·HOME만 받는다', () => {
    const output = 't1\t\tcodex\x1f\tHOME=/home/me\x1fCODEX_HOME=/home/me/.paddock/agent-profiles/a/codex\x1fOPENAI_API_KEY=secret\x1f\r\n';
    assert.deepEqual(wsl.parse(output).t1, { cwd: '', argv: ['codex'], env: { HOME: '/home/me', CODEX_HOME: '/home/me/.paddock/agent-profiles/a/codex' }, distribution: '', sessionRecord: '', pid: '' });
    // 스크립트는 앞쪽 프로그램의 환경에서 세 이름만 골라 출력한다.
    assert.match(wsl.script(), /grep -E '\^\(CLAUDE_CONFIG_DIR\|CODEX_HOME\|HOME\)='/);
    assert.match(wsl.script(), /\/proc\/\$fg\/environ/);
});

test('C-wsl.4: 다섯째 칸은 셸의 WSL 배포판 이름이고, 계정 판정에서 배포판을 비교하는 데 쓴다', () => {
    const output = 't1\t\tcodex\x1f\tHOME=/home/me\x1f\tUbuntu-22.04\r\n';
    assert.deepEqual(wsl.parse(output).t1, { cwd: '', argv: ['codex'], env: { HOME: '/home/me' }, distribution: 'Ubuntu-22.04', sessionRecord: '', pid: '' });
    assert.match(wsl.script(), /WSL_DISTRO_NAME=/);
});

test('C-wsl.forward: WSL 터미널은 사용자 WSLENV를 지키며 표지와 COLORTERM을 넘긴다', () => {
    assert.equal(wsl.forwardedWslEnv(''), 'PADDOCK_TERMINAL:COLORTERM');
    assert.equal(wsl.forwardedWslEnv('USERPROFILE/p'), 'USERPROFILE/p:PADDOCK_TERMINAL:COLORTERM');
    assert.equal(wsl.forwardedWslEnv('COLORTERM/u:PADDOCK_TERMINAL'), 'COLORTERM/u:PADDOCK_TERMINAL');
});

test('C-wsl.mem.1: 메모리 줄은 터미널별 고유 RSS를 더하고 터미널 목록(parse)에는 섞이지 않는다', () => {
    const output = [
        '#memory\t100\t40\t10\t4',
        '#rss\tt1\t3',
        '#rss\tt1\t2',
        '#rss\tt2\t1',
        't1\t\tbash\x1f\t\t',
    ].join('\n');
    assert.deepEqual(wsl.parseMemory(output), { total: 102400, available: 40960, swap: 10240, swapUsed: 6144, terminals: { t1: 5 * 4096, t2: 4096 } });
    assert.deepEqual(Object.keys(wsl.parse(output)), ['t1']);
});

test('C-wsl.mem.2: 메모리 줄이 없거나 깨졌으면 null이다', () => {
    assert.equal(wsl.parseMemory(''), null);
    assert.equal(wsl.parseMemory('t1\t\tbash\n'), null);
    assert.equal(wsl.parseMemory('#memory\t\t\t\t\n'), null);
    assert.equal(wsl.parseMemory('#memory\t100\t40\t0\n'), null);
});

test('C-wsl.mem.3: 스크립트는 meminfo와 표지 프로세스의 statm을 내장 명령만으로 읽는다', () => {
    assert.match(wsl.script(), /\/proc\/meminfo/);
    assert.match(wsl.script(), /\/proc\/\$p\/statm/);
    assert.doesNotMatch(wsl.script(), /getconf/);
});

test('C-wsl.5: 여섯째·일곱째 칸은 앞쪽 프로세스의 sessions 기록 본문과 pid이고, 스크립트가 기록 본문을 그대로 읽는다', () => {
    const record = '{"pid":55,"sessionId":"0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10","kind":"interactive"}';
    const parsed = wsl.parse(`t1\t\tclaude\x1f\tHOME=/home/me\x1f\tUbuntu\t${record}\t55\r\n`).t1;
    assert.equal(parsed.sessionRecord, record);
    assert.equal(parsed.pid, '55');

    // 스크립트에서 기록을 읽는 줄들만 떼어 실제 sh로 돌린다: 계정 폴더가 있으면 그 폴더, 없으면 HOME/.claude의 기록을 읽는다.
    const lines = wsl.script().split('\n').filter(line => /^\s+(cfg|hm|sess)=/.test(line));
    assert.equal(lines.length, 3);
    const home = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-wsl-sess-'));
    fs.mkdirSync(path.join(home, '.claude', 'sessions'), { recursive: true });
    fs.mkdirSync(path.join(home, 'account', 'sessions'), { recursive: true });
    fs.writeFileSync(path.join(home, '.claude', 'sessions', '55.json'), record);
    fs.writeFileSync(path.join(home, 'account', 'sessions', '55.json'), '{"pid":55,\n"sessionId":"22222222-2222-4222-8222-222222222222","kind":"sdk"}');
    const read = env => childProcess.execFileSync('sh', ['-c', `fg=55; env='${env}'; ${lines.join('\n')}\nprintf %s "$sess"`], { encoding: 'utf8' });
    assert.equal(read(`HOME=${home}\x1f`), record);
    // 비대화형(sdk) 기록도 본문은 그대로 오고, 거르는 일은 읽는 쪽 판정이 한다. 줄바꿈은 지워진다.
    assert.equal(read(`CLAUDE_CONFIG_DIR=${home}/account\x1fHOME=${home}\x1f`), '{"pid":55,"sessionId":"22222222-2222-4222-8222-222222222222","kind":"sdk"}');
    assert.equal(read(`HOME=${home}/nowhere\x1f`), '');
});
