// 이 컴퓨터에서 앱을 띄울 준비가 안 됐으면 먼저 한다. 피드백 실행 전에 호출한다.
// 의존성이 없어도 돌아가야 하므로 Node 기본 모듈만 쓴다.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const desktopDirectory = path.join(__dirname, '..');
const projectRoot = path.join(desktopDirectory, '..');

function runNpm(
    script,
) {
    // npm은 Windows에서 npm.cmd라 셸을 거쳐 실행한다.
    const result = spawnSync(`npm run ${script}`, { cwd: projectRoot, stdio: 'inherit', shell: true });
    if (result.status !== 0) {
        process.exit(result.status ?? 1);
    }
}

if (process.platform === 'linux' && /microsoft/i.test(os.release())) {
    console.warn('WSL에서는 Linux용 앱으로 실행됩니다. Windows용 앱은 PowerShell이나 명령 프롬프트에서 npm run feedback으로 띄우세요.');
}
// Step 1: 의존성 설치와 네이티브 모듈의 Electron용 컴파일. 컴파일이 끝나면 theia rebuild가 원본을 .browser_modules에 보관한다.
// Source control 보기에 쓰는 git 확장(plugins/)도 setup이 받는다.
const isPrepared = fs.existsSync(path.join(desktopDirectory, '.browser_modules', 'modules.json')) && fs.existsSync(path.join(desktopDirectory, 'plugins'));
if (!isPrepared) {
    console.log('처음 실행이라 의존성을 설치하고 이 운영체제용으로 컴파일합니다.');
    runNpm('setup');
}
// Step 2: 화면 번들. 설치 파일을 만든 뒤라면 배포용 번들이 이미 있어 그대로 쓴다.
if (!fs.existsSync(path.join(desktopDirectory, 'lib', 'frontend', 'index.html'))) {
    console.log('앱 화면을 빌드합니다.');
    runNpm('build');
}
