const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { prepareLinuxIme } = require('./linux-ime.cjs');

// 확장·설정 폴더는 herdr-shell/electron-main-module.js가 앱 시작 시 정한다. 설치본도 같은 경로를 쓴다.
const desktopDirectory = path.join(__dirname, '..');
const cli = path.join(desktopDirectory, 'node_modules', '@theia', 'cli', 'bin', 'theia.js');
if (!fs.existsSync(cli) || !fs.existsSync(path.join(desktopDirectory, 'lib', 'frontend', 'index.html'))) {
    console.error('실행 파일이 준비되지 않았습니다. 저장소 루트에서 npm run setup과 npm run build를 실행하세요.');
    process.exit(1);
}

const ime = prepareLinuxIme();
if (ime.message) console.log(ime.message);

// node_modules/.bin/theia는 Windows에서 .cmd 래퍼라 셸 없이 실행할 수 없다. CLI 스크립트를 Node로 직접 실행한다.
const child = spawn(process.execPath, [
    cli,
    'start',
    ...process.argv.slice(2),
], {
    cwd: desktopDirectory,
    stdio: 'inherit',
    env: { ...process.env, ...ime.env },
});

child.on('error', (error) => {
    console.error(`Herdr Terminal을 실행할 수 없습니다: ${error.message}`);
    process.exitCode = 1;
});

for (const signal of ['SIGINT', 'SIGTERM']) {
    process.on(signal, () => child.kill(signal));
}
child.on('exit', (code, signal) => {
    process.exitCode = code === null ? (signal ? 1 : 0) : code;
});
