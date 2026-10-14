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
