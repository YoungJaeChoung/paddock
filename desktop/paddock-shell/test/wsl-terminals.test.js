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
        t1: { cwd: '\\\\wsl.localhost\\Ubuntu\\home\\me\\app', argv: ['node', '/usr/bin/claude'] },
        t2: { cwd: '', argv: ['-bash'] },
    });
    assert.deepEqual(wsl.parse(''), {});
});

test('C-wsl.forward: WSL 터미널은 사용자 WSLENV를 지키며 표지와 COLORTERM을 넘긴다', () => {
    assert.equal(wsl.forwardedWslEnv(''), 'PADDOCK_TERMINAL:COLORTERM');
    assert.equal(wsl.forwardedWslEnv('USERPROFILE/p'), 'USERPROFILE/p:PADDOCK_TERMINAL:COLORTERM');
    assert.equal(wsl.forwardedWslEnv('COLORTERM/u:PADDOCK_TERMINAL'), 'COLORTERM/u:PADDOCK_TERMINAL');
});
