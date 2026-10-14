// WSL 저장소용 git 분배기(paddock-shell/git-shim/PaddockGit.cs)를 paddock-shell/bin/git.exe로 만든다.
// 앱 실행(start.cjs)과 설치 파일 제작(package.cjs)이 부른다. Windows가 아니면 아무것도 하지 않는다.
// 컴파일러는 Windows에 기본으로 들어 있는 .NET Framework의 csc.exe라 따로 설치할 도구가 없다.
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const desktopDirectory = path.join(__dirname, '..');
const SOURCE = path.join(desktopDirectory, 'paddock-shell', 'git-shim', 'PaddockGit.cs');
const OUTPUT = path.join(desktopDirectory, 'paddock-shell', 'bin', 'git.exe');

/** .NET Framework 4.x의 csc.exe 위치. 64비트 폴더를 먼저 보고 없으면 null. */
function findCompiler(
    windowsDirectory,
) {
    const candidates = [
        path.join(windowsDirectory, 'Microsoft.NET', 'Framework64', 'v4.0.30319', 'csc.exe'),
        path.join(windowsDirectory, 'Microsoft.NET', 'Framework', 'v4.0.30319', 'csc.exe'),
    ];
    return candidates.find(candidate => fs.existsSync(candidate)) ?? null;
}

/**
 * 분배기를 만들고 결과를 돌려준다. 소스보다 새 출력이 이미 있으면 다시 만들지 않는다.
 *
 * Returns
 * -------
 * { status, output, message }
 *     status는 'built' | 'fresh' | 'skipped'(Windows 아님) | 'failed'(컴파일러 없음·컴파일 실패).
 *     실패해도 앱은 실행할 수 있다(Git 보기가 WSL 폴더에서 Windows git으로 돌아갈 뿐이다).
 */
function buildGitShim() {
    let result = { status: 'skipped', output: OUTPUT, message: 'Windows가 아니라 건너뜁니다.' };
    if (process.platform === 'win32') {
        const compiler = findCompiler(process.env.WINDIR || 'C:\\Windows');
        const fresh = fs.existsSync(OUTPUT) && fs.statSync(OUTPUT).mtimeMs >= fs.statSync(SOURCE).mtimeMs;
        if (fresh) {
            result = { status: 'fresh', output: OUTPUT, message: '' };
        } else if (compiler === null) {
            result = { status: 'failed', output: OUTPUT, message: '.NET Framework의 csc.exe를 찾지 못했습니다.' };
        } else {
            fs.mkdirSync(path.dirname(OUTPUT), { recursive: true });
            const compile = spawnSync(compiler, ['/nologo', '/optimize', '/target:exe', '/codepage:65001', `/out:${OUTPUT}`, SOURCE], { encoding: 'utf8' });
            result = compile.status === 0
                ? { status: 'built', output: OUTPUT, message: '' }
                : { status: 'failed', output: OUTPUT, message: `${compile.stdout}${compile.stderr}`.trim() };
        }
    }
    return result;
}

module.exports = { buildGitShim, findCompiler, SOURCE, OUTPUT };

if (require.main === module) {
    const result = buildGitShim();
    if (result.status === 'failed') {
        console.error(`git 분배기를 만들지 못했습니다: ${result.message}`);
        process.exitCode = 1;
    } else if (result.message) {
        console.log(result.message);
    }
}
