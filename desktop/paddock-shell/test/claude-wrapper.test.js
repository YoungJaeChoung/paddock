const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { ClaudeWrapper, claudeWrapperScript, installClaudeWrapper, withWrapperPath } = require('../claude-wrapper');

function folder(
    context,
) {
    const directory = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'claude wrapper '));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    return directory;
}

/** 진짜 claude 자리에 인자를 그대로 출력하는 가짜 실행 파일을 둔다. */
function fakeClaude(
    directory,
) {
    fs.mkdirSync(directory, { recursive: true });
    fs.writeFileSync(path.join(directory, 'claude'), '#!/bin/sh\nprintf "REAL|%s\\n" "$*"\nexit 7\n', { mode: 0o755 });
}

test('C-wrapper.1: sh 래퍼 본문은 셸 문법 검사를 통과한다', { skip: process.platform === 'win32' }, () => {
    const result = spawnSync('sh', ['-n', '-c', claudeWrapperScript({ kind: 'sh', flagFile: "/tmp/it's flag.json" })], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
});

test('C-wrapper.2: 래퍼는 자기 폴더를 PATH에서 빼고 진짜 claude를 플래그 파일과 함께 실행하며 종료 코드를 그대로 낸다', { skip: process.platform === 'win32' }, context => {
    const root = folder(context);
    const bin = path.join(root, 'config', 'bin');
    const real = path.join(root, 'real bin');
    fakeClaude(real);
    const flagFile = path.join(root, "flag's settings.json");
    fs.writeFileSync(flagFile, '{}');
    const wrapper = installClaudeWrapper({ directory: bin, kind: 'sh', flagFile });
    assert.equal(wrapper, path.join(bin, 'claude'));
    if (process.platform !== 'win32') assert.equal(fs.statSync(wrapper).mode & 0o777, 0o700);
    // 래퍼 폴더가 PATH 맨 앞에 있어도 자기 자신을 다시 부르지 않고(무한 반복 없음) 진짜 claude에 닿는다.
    const environment = { PATH: `${bin}:${real}:/usr/bin:/bin` };
    const found = spawnSync('sh', ['-c', 'command -v claude'], { encoding: 'utf8', env: environment });
    assert.equal(found.stdout.trim(), wrapper);
    const result = spawnSync('claude', ['-p', 'two words', '--model', 'x'], { encoding: 'utf8', env: environment, timeout: 10000 });
    assert.equal(result.status, 7);
    assert.equal(result.stdout.trim(), `REAL|--settings ${flagFile} -p two words --model x`);
});

test('C-wrapper.3: 플래그 파일이 없으면 인자를 붙이지 않고 그대로 넘긴다', { skip: process.platform === 'win32' }, context => {
    const root = folder(context);
    const bin = path.join(root, 'bin');
    const real = path.join(root, 'real');
    fakeClaude(real);
    installClaudeWrapper({ directory: bin, kind: 'sh', flagFile: path.join(root, 'missing.json') });
    const result = spawnSync('claude', ['--version'], { encoding: 'utf8', env: { PATH: `${bin}:${real}:/usr/bin:/bin` } });
    assert.equal(result.stdout.trim(), 'REAL|--version');
});

test('C-wrapper.4: 진짜 claude가 PATH에 없으면 래퍼 자신이 아니라 not found로 끝난다', { skip: process.platform === 'win32' }, context => {
    const root = folder(context);
    const bin = path.join(root, 'bin');
    installClaudeWrapper({ directory: bin, kind: 'sh', flagFile: path.join(root, 'missing.json') });
    const result = spawnSync('claude', [], { encoding: 'utf8', env: { PATH: `${bin}:/usr/bin:/bin` }, timeout: 10000 });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /not found/);
});

test('C-wrapper.5: 같은 내용이면 다시 쓰지 않고, 옛 내용이면 교체하며, 링크·폴더 자리는 거부한다', context => {
    const root = folder(context);
    const bin = path.join(root, 'bin');
    const options = { directory: bin, kind: 'sh', flagFile: '/x/flag.json' };
    const file = installClaudeWrapper(options);
    const inode = fs.statSync(file).ino;
    installClaudeWrapper(options);
    assert.equal(fs.statSync(file).ino, inode);
    fs.writeFileSync(file, '#!/bin/sh\n# old\n');
    installClaudeWrapper(options);
    assert.equal(fs.readFileSync(file, 'utf8'), claudeWrapperScript({ kind: 'sh', flagFile: '/x/flag.json' }));
    assert.deepEqual(fs.readdirSync(bin), ['claude']);
    fs.rmSync(file);
    fs.symlinkSync('/bin/true', file);
    assert.throws(() => installClaudeWrapper(options), /not a regular file/);
});

test('C-wrapper.6: cmd 래퍼는 자기 폴더를 PATH에서 빼고 플래그 파일이 있을 때만 --settings를 붙인다', () => {
    const script = claudeWrapperScript({ kind: 'cmd', flagFile: 'C:\\Users\\me\\.paddock\\config\\claude\\flag-settings.json', ownDirectory: 'C:\\Users\\me\\.paddock\\config\\bin' });
    assert.ok(script.includes('set "PATH=%PATH:C:\\Users\\me\\.paddock\\config\\bin;=%"'));
    assert.ok(script.includes('set "PATH=%PATH:;C:\\Users\\me\\.paddock\\config\\bin=%"'));
    assert.ok(script.includes('if not exist "C:\\Users\\me\\.paddock\\config\\claude\\flag-settings.json" goto plain'));
    assert.ok(script.includes('claude --settings "C:\\Users\\me\\.paddock\\config\\claude\\flag-settings.json" %*'));
    assert.ok(script.includes('\r\nclaude %*\r\n'));
    assert.equal(ClaudeWrapper.FILES.cmd, 'claude.cmd');
});

test('C-wrapper.7: 터미널 PATH 맨 앞에 래퍼 폴더를 붙이고 이미 앞에 있으면 다시 붙이지 않으며 입력 환경은 바꾸지 않는다', () => {
    const env = { PATH: '/usr/bin:/bin', HOME: '/h' };
    const next = withWrapperPath(env, '/p/bin', { baseEnv: {}, isWindows: false });
    assert.equal(next.PATH, '/p/bin:/usr/bin:/bin');
    assert.equal(next.HOME, '/h');
    assert.equal(env.PATH, '/usr/bin:/bin');
    assert.equal(withWrapperPath(next, '/p/bin', { baseEnv: {}, isWindows: false }).PATH, '/p/bin:/usr/bin:/bin');
    assert.equal(withWrapperPath(undefined, '/p/bin', { baseEnv: { PATH: '/x' }, isWindows: false }).PATH, '/p/bin:/x');
    assert.equal(withWrapperPath({}, '/p/bin', { baseEnv: {}, isWindows: false }).PATH, '/p/bin');
});

test('C-wrapper.8: Windows는 Path 키 이름을 유지하고 세미콜론으로 잇는다', () => {
    const next = withWrapperPath({ Path: 'C:\\a;C:\\b' }, 'C:\\p\\bin', { baseEnv: {}, isWindows: true });
    assert.deepEqual(Object.keys(next), ['Path']);
    assert.equal(next.Path, 'C:\\p\\bin;C:\\a;C:\\b');
    assert.equal(withWrapperPath(undefined, 'C:\\p\\bin', { baseEnv: { Path: 'C:\\x' }, isWindows: true }).Path, 'C:\\p\\bin;C:\\x');
});
