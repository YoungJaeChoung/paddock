// WSL 폴더용 git 분배기(PaddockGit.cs)를 실제로 컴파일해 실행하는 통합 시험.
// Windows 컴파일러(csc.exe)와 WSL이 함께 있는 환경(WSL의 Linux Node)에서만 돈다. 그 밖에서는 건너뛴다.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { findCompiler, SOURCE } = require('../../scripts/build-git-shim.cjs');

const isWsl = process.platform === 'linux' && /microsoft/i.test(os.release());
const compiler = isWsl ? findCompiler('/mnt/c/Windows') : null;
const skip = compiler === null ? 'WSL과 Windows csc.exe가 필요합니다' : false;

/** 분배기를 임시 폴더에 컴파일하고, 그 폴더 아래에 시험용 저장소를 만든다. */
function prepare(
    context,
) {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-git-shim-'));
    context.after(() => fs.rmSync(root, { recursive: true, force: true }));
    const windowsPath = file => spawnSync('wslpath', ['-w', file], { encoding: 'utf8' }).stdout.trim();
    const exe = path.join(root, 'git.exe');
    const compile = spawnSync(compiler, ['/nologo', '/codepage:65001', `/out:${windowsPath(exe)}`, windowsPath(SOURCE)], { encoding: 'utf8' });
    assert.equal(compile.status, 0, compile.stdout + compile.stderr);
    fs.chmodSync(exe, 0o755);
    const repository = path.join(root, 'repo');
    fs.mkdirSync(repository);
    spawnSync('git', ['init', '-q'], { cwd: repository });
    return { exe, repository, windowsPath, distro: process.env.WSL_DISTRO_NAME };
}

/** WSL git에게 보이는 환경 변수 값. 없으면 빈 줄. 분배기가 WSLENV로 넘긴 것만 보인다. */
function seen(
    exe,
    repository,
    name,
    env,
) {
    return run(exe, ['-c', `alias.e=!printenv ${name}`, 'e'], { cwd: repository, env: { ...process.env, ...env } }).out;
}

function run(
    exe,
    args,
    options,
) {
    const result = spawnSync(exe, args, { encoding: 'utf8', ...options });
    return { status: result.status, out: result.stdout, err: result.stderr };
}

test('arguments reach the WSL git unchanged', { skip }, context => {
    const { exe, repository } = prepare(context);
    const args = ['a b', '한글 파일.txt', 'say "hi"', 'back\\slash', 'trail\\', '', '$HOME', '*', "it's", '--'];
    const expected = spawnSync('git', ['rev-parse', '--sq-quote', ...args], { encoding: 'utf8' }).stdout;
    assert.equal(run(exe, ['rev-parse', '--sq-quote', ...args], { cwd: repository }).out, expected);
});

test('Windows paths in arguments become Linux paths', { skip }, context => {
    const { exe, repository, distro } = prepare(context);
    const unc = `\\\\wsl.localhost\\${distro}\\home\\me\\a b.txt`;
    const other = '\\\\wsl.localhost\\SomeOtherDistro\\x';
    const { out } = run(exe, ['rev-parse', '--sq-quote', 'C:\\Users\\me\\x y.patch', 'D:/data/f', '--git-dir=C:/r/.git', unc, other, 'HEAD:C:x', 'C:'], { cwd: repository });
    assert.equal(out, " '/mnt/c/Users/me/x y.patch' '/mnt/d/data/f' '--git-dir=/mnt/c/r/.git' '/home/me/a b.txt' '\\\\wsl.localhost\\SomeOtherDistro\\x' 'HEAD:C:x' 'C:'\n");
});

test('paths in the output are returned in the form Windows git prints', { skip }, context => {
    const { exe, repository, distro } = prepare(context);
    const { out, status } = run(exe, ['rev-parse', '--show-toplevel', '--git-dir', '--git-common-dir'], { cwd: repository });
    assert.equal(status, 0);
    assert.equal(out, `//wsl.localhost/${distro}${repository}\n.git\n.git\n`);
    assert.equal(run(exe, ['rev-parse', '--absolute-git-dir'], { cwd: repository }).out, `//wsl.localhost/${distro}${repository}/.git\n`);
});

test('stdin, binary output and exit codes pass through', { skip }, context => {
    const { exe, repository } = prepare(context);
    const bytes = Buffer.from(Array.from({ length: 256 * 40 }, (_, index) => index % 256));
    const hashed = spawnSync(exe, ['hash-object', '-w', '--stdin'], { cwd: repository, input: bytes, encoding: 'utf8' });
    assert.equal(hashed.status, 0);
    const blob = spawnSync(exe, ['cat-file', 'blob', hashed.stdout.trim()], { cwd: repository });
    assert.ok(blob.stdout.equals(bytes));
    assert.equal(run(exe, ['rev-parse', '--verify', 'nonexistent'], { cwd: repository }).status, 128);
    assert.equal(run(exe, ['diff', '--exit-code', '--no-index', '/dev/null', '/dev/null'], { cwd: repository }).status, 0);
});

test('outside WSL folders the Windows git runs', { skip }, context => {
    const { exe } = prepare(context);
    const { out, status } = run(exe, ['--version'], { cwd: '/mnt/c' });
    assert.equal(status, 0);
    assert.match(out, /^git version .*windows/);
});

test('rebase-style editor "true" reaches WSL git, Windows-path editors send the call to Windows git', { skip }, context => {
    const { exe, repository } = prepare(context);
    const forwarded = 'GIT_EDITOR:GIT_SEQUENCE_EDITOR';
    // 단순 명령은 WSL 안 git이 받는다(WSL git 버전, Windows 표지 없음).
    const simple = { GIT_EDITOR: 'true', GIT_SEQUENCE_EDITOR: ':', WSLENV: forwarded };
    assert.equal(seen(exe, repository, 'GIT_EDITOR', simple), 'true\n');
    assert.equal(seen(exe, repository, 'GIT_SEQUENCE_EDITOR', simple), ':\n');
    assert.doesNotMatch(run(exe, ['--version'], { cwd: repository, env: { ...process.env, ...simple } }).out, /windows/);
    // 확장 폴더의 Windows 스크립트면 그 호출만 Windows git이 실행한다.
    const script = { GIT_EDITOR: 'C:\\ext\\git-editor.sh', WSLENV: forwarded };
    assert.match(run(exe, ['--version'], { cwd: repository, env: { ...process.env, ...script } }).out, /windows/);
    const sequence = { GIT_SEQUENCE_EDITOR: 'C:\\ext\\git-editor.sh', WSLENV: forwarded };
    assert.match(run(exe, ['--version'], { cwd: repository, env: { ...process.env, ...sequence } }).out, /windows/);
});

test('askpass variables and Windows-only settings never reach WSL git', { skip }, context => {
    const { exe, repository } = prepare(context);
    const env = {
        GIT_ASKPASS: '/x/askpass.sh',
        SSH_ASKPASS: '/x/ssh.sh',
        VSCODE_GIT_ASKPASS_NODE: '/x/node',
        VSCODE_GIT_IPC_HANDLE: '/x/ipc',
        ELECTRON_RUN_AS_NODE: '1',
        // 사용자가 WSLENV에 이미 올려 둔 것도 걸러 낸다.
        WSLENV: 'GIT_ASKPASS:SSH_ASKPASS:VSCODE_GIT_ASKPASS_NODE:VSCODE_GIT_IPC_HANDLE:ELECTRON_RUN_AS_NODE',
    };
    for (const name of ['GIT_ASKPASS', 'SSH_ASKPASS', 'VSCODE_GIT_ASKPASS_NODE', 'VSCODE_GIT_IPC_HANDLE', 'ELECTRON_RUN_AS_NODE']) {
        assert.equal(seen(exe, repository, name, env), '', name);
    }
});

test('extension variables are forwarded and terminal prompts are disabled', { skip }, context => {
    const { exe, repository } = prepare(context);
    const env = { GIT_OPTIONAL_LOCKS: '0', GIT_PAGER: 'cat', VSCODE_GIT_COMMAND: 'status', LC_ALL: 'en_US.UTF-8', WSLENV: 'GIT_OPTIONAL_LOCKS:GIT_PAGER:VSCODE_GIT_COMMAND:LC_ALL' };
    assert.equal(seen(exe, repository, 'GIT_OPTIONAL_LOCKS', env), '0\n');
    assert.equal(seen(exe, repository, 'GIT_PAGER', env), 'cat\n');
    assert.equal(seen(exe, repository, 'VSCODE_GIT_COMMAND', env), 'status\n');
    assert.equal(seen(exe, repository, 'GIT_TERMINAL_PROMPT', env), '0\n');
});

test('worktree list output paths are returned in the Windows git form', { skip }, context => {
    const { exe, repository, distro } = prepare(context);
    spawnSync('git', ['-c', 'user.email=a@b', '-c', 'user.name=t', 'commit', '-q', '--allow-empty', '-m', 'i'], { cwd: repository });
    const root = `//wsl.localhost/${distro}${repository}`;
    const porcelain = run(exe, ['worktree', 'list', '--porcelain'], { cwd: repository }).out;
    assert.ok(porcelain.startsWith(`worktree ${root}\nHEAD `), porcelain);
    assert.match(run(exe, ['worktree', 'list'], { cwd: repository }).out, new RegExp(`^${root.replace(/[.$]/g, '\\$&')} +[0-9a-f]+ \\[`));
});

test('output keeps the notation of the current folder (wsl$ versus wsl.localhost)', { skip: skip || !fs.existsSync('/mnt/c/Program Files/nodejs/node.exe') }, context => {
    const { exe, repository, windowsPath, distro } = prepare(context);
    const unc = windowsPath(repository).replace('wsl.localhost', 'wsl$');
    const script = 'const r=require("child_process").spawnSync(process.argv[1],["rev-parse","--show-toplevel"],{cwd:process.argv[2],encoding:"utf8"});process.stdout.write(r.stdout)';
    const out = spawnSync('/mnt/c/Program Files/nodejs/node.exe', ['-e', script, windowsPath(exe), unc], { encoding: 'utf8' }).stdout;
    assert.equal(out, `//wsl$/${distro}${repository}\n`);
});
