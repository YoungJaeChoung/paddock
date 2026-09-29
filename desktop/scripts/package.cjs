// 현재 운영체제용 Paddock 설치 파일을 desktop/dist/에 만든다.
// 사용: node scripts/package.cjs mac | win
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { ensurePlugins } = require('./ensure-plugins.cjs');

const TARGET = {
    mac: { platform: 'darwin', name: 'macOS', builderFlag: '--mac' },
    win: { platform: 'win32', name: 'Windows', builderFlag: '--win' },
};

const desktopDirectory = path.join(__dirname, '..');
const target = TARGET[process.argv[2]];
if (!target) {
    console.error('설치 대상을 지정하세요: node scripts/package.cjs mac | win');
    process.exit(1);
}
// 네이티브 모듈을 다른 운영체제용으로 교차 컴파일할 수 없으므로 대상 운영체제에서만 만든다.
if (process.platform !== target.platform) {
    console.error(`${target.name} 설치 파일은 ${target.name}에서 만들어야 합니다. 현재 운영체제: ${process.platform}`);
    process.exit(1);
}

function runNode(
    script,
    args,
) {
    const result = spawnSync(process.execPath, [script, ...args], { cwd: desktopDirectory, stdio: 'inherit' });
    if (result.status !== 0) {
        process.exit(result.status ?? 1);
    }
}

// Step 0: 확장이 빠진 채 설치 파일을 만들지 않는다. 받지 못하면 멈춘다(코드 색·이미지 미리보기가 없는 설치본을 막는다).
const missingPlugins = ensurePlugins();
if (missingPlugins.length) {
    console.error(`확장 ${missingPlugins.length}개를 받지 못해 설치 파일을 만들지 않습니다: ${missingPlugins.join(', ')}`);
    process.exit(1);
}
// Step 1: 설치본에는 압축된 배포용 번들을 넣는다.
runNode(path.join(desktopDirectory, 'node_modules', '@theia', 'cli', 'bin', 'theia.js'), ['build', '--mode', 'production']);
// Step 2: electron-builder로 설치 파일을 만든다. 설정은 electron-builder.yml에 있다.
runNode(path.join(desktopDirectory, 'node_modules', 'electron-builder', 'cli.js'), [target.builderFlag, '--publish', 'never']);
console.log(`\n${target.name} 설치 파일: ${path.join(desktopDirectory, 'dist')}`);
