const test = require('node:test');
const assert = require('node:assert/strict');
const wsl = require('../wsl-terminals');

test('C-wsl.1: wsl.exe 셸만 WSL 터미널로 본다', () => {
    assert.equal(wsl.isWslShell('C:\\Windows\\System32\\wsl.exe'), true);
    assert.equal(wsl.isWslShell('wsl'), true);
    assert.equal(wsl.isWslShell('C:\\Program Files\\Git\\bin\\bash.exe'), false);
    assert.equal(wsl.isWslShell(undefined), false);
});

test('C-wsl.2: 스크립트 출력에서 터미널별 현재 폴더와 명령줄을 읽고 깨진 줄은 버린다', () => {
    const output = 't1\t\\\\wsl.localhost\\Ubuntu\\home\\me\\app\tnode\x1f/usr/bin/claude\x1f\r\nt2\t\t-bash\x1f\nbroken line\n';
    assert.deepEqual(wsl.parse(output), {
        t1: { cwd: '\\\\wsl.localhost\\Ubuntu\\home\\me\\app', argv: ['node', '/usr/bin/claude'], env: {}, distribution: '' },
        t2: { cwd: '', argv: ['-bash'], env: {}, distribution: '' },
    });
    assert.deepEqual(wsl.parse(''), {});
});

test('C-wsl.3: 넷째 칸의 계정 환경은 CODEX_HOME·CLAUDE_CONFIG_DIR·HOME만 받는다', () => {
    const output = 't1\t\tcodex\x1f\tHOME=/home/me\x1fCODEX_HOME=/home/me/.paddock/agent-profiles/a/codex\x1fOPENAI_API_KEY=secret\x1f\r\n';
    assert.deepEqual(wsl.parse(output).t1, { cwd: '', argv: ['codex'], env: { HOME: '/home/me', CODEX_HOME: '/home/me/.paddock/agent-profiles/a/codex' }, distribution: '' });
    // 스크립트는 앞쪽 프로그램의 환경에서 세 이름만 골라 출력한다.
    assert.match(wsl.script(), /grep -E '\^\(CLAUDE_CONFIG_DIR\|CODEX_HOME\|HOME\)='/);
    assert.match(wsl.script(), /\/proc\/\$fg\/environ/);
});

test('C-wsl.4: 다섯째 칸은 셸의 WSL 배포판 이름이고, 계정 판정에서 배포판을 비교하는 데 쓴다', () => {
    const output = 't1\t\tcodex\x1f\tHOME=/home/me\x1f\tUbuntu-22.04\r\n';
    assert.deepEqual(wsl.parse(output).t1, { cwd: '', argv: ['codex'], env: { HOME: '/home/me' }, distribution: 'Ubuntu-22.04' });
    assert.match(wsl.script(), /WSL_DISTRO_NAME=/);
});

test('C-wsl.forward: WSL 터미널은 사용자 WSLENV를 지키며 표지와 COLORTERM을 넘긴다', () => {
    assert.equal(wsl.forwardedWslEnv(''), 'PADDOCK_TERMINAL:COLORTERM');
    assert.equal(wsl.forwardedWslEnv('USERPROFILE/p'), 'USERPROFILE/p:PADDOCK_TERMINAL:COLORTERM');
    assert.equal(wsl.forwardedWslEnv('COLORTERM/u:PADDOCK_TERMINAL'), 'COLORTERM/u:PADDOCK_TERMINAL');
});
