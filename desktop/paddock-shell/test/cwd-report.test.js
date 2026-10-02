const test = require('node:test');
const assert = require('node:assert/strict');
const report = require('../cwd-report');

test('C-cwd.1: 셸마다 현재 폴더 알림을 켜는 옵션을 고르고, 모르는 셸은 null이다', () => {
    const bash = report.cwdReportOptions('C:\\Program Files\\Git\\bin\\bash.exe', ['--login', '-i']);
    assert.match(bash.env.PROMPT_COMMAND, /\]7;/);
    assert.match(bash.env.PROMPT_COMMAND, /trap __paddock_cwd DEBUG/);
    assert.deepEqual(bash.shellArgs, ['--login', '-i']);
    const powershell = report.cwdReportOptions('C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe', ['-NoLogo']);
    assert.deepEqual(powershell.shellArgs.slice(0, 3), ['-NoLogo', '-NoExit', '-EncodedCommand']);
    assert.equal(Buffer.from(powershell.shellArgs[3], 'base64').toString('utf16le').includes('function global:prompt'), true);
    assert.equal(Buffer.from(powershell.shellArgs[3], 'base64').toString('utf16le').includes('Microsoft.PowerShell.Management\\$name @args'), true);
    assert.equal(report.cwdReportOptions('C:\\Windows\\System32\\cmd.exe').env.PROMPT, '$E]7;$P$E\\$P$G');
    assert.equal(report.cwdReportOptions('C:\\Windows\\System32\\wsl.exe'), null);
});

test('C-cwd.2: 알림 내용을 슬래시 Windows 경로로 바꾼다', () => {
    assert.equal(report.parseCwdReport('C:/Users/me/app'), 'C:/Users/me/app');
    assert.equal(report.parseCwdReport('C:\\Users\\me\\app'), 'C:/Users/me/app');
    assert.equal(report.parseCwdReport('file://host/C:/Users/me/a%20b'), 'C:/Users/me/a b');
    assert.equal(report.parseCwdReport('/c/Users/me/app'), 'C:/Users/me/app');
    assert.equal(report.parseCwdReport('/d'), 'D:/');
    assert.equal(report.parseCwdReport('/home/me'), '/home/me');
    assert.equal(report.parseCwdReport(''), '');
});
