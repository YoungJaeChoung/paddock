const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { prepareLinuxIme } = require('./linux-ime.cjs');
const { ensurePlugins } = require('./ensure-plugins.cjs');

// 확장·설정 폴더는 paddock-shell/electron-main-module.js가 앱 시작 시 정한다. 설치본도 같은 경로를 쓴다.
const desktopDirectory = path.join(__dirname, '..');
const cli = path.join(desktopDirectory, 'node_modules', '@theia', 'cli', 'bin', 'theia.js');
if (!fs.existsSync(cli) || !fs.existsSync(path.join(desktopDirectory, 'lib', 'frontend', 'index.html'))) {
    console.error('실행 파일이 준비되지 않았습니다. 저장소 루트에서 npm run setup과 npm run build를 실행하세요.');
    process.exit(1);
}

// 코드를 새로 받은 뒤 setup을 안 돌렸어도 새로 추가된 확장(언어 색·미리보기)이 빠지지 않게 채운다. 오프라인이면 경고만 하고 계속한다.
const missingPlugins = ensurePlugins();
if (missingPlugins.length) console.warn(`확장 ${missingPlugins.length}개를 받지 못했습니다(네트워크 확인). 코드 색·이미지 미리보기 같은 기능이 빠질 수 있습니다: ${missingPlugins.join(', ')}`);

const ime = prepareLinuxIme();
if (ime.message) console.log(ime.message);

// npm으로 실행하면(npm start, npm --prefix ... run) npm이 npm_config_prefix 같은 자기 변수를 환경에 넣는다.
// 이 값이 앱의 터미널 셸까지 물려 가면 nvm이 "npm_config_prefix와 호환되지 않는다"는 경고를 띄우고 동작을 바꾸므로 걷어 낸다.
const appEnv = Object.fromEntries(Object.entries({ ...process.env, ...ime.env }).filter(([name]) => !/^npm_/i.test(name) && name !== 'INIT_CWD'));

// node_modules/.bin/theia는 Windows에서 .cmd 래퍼라 셸 없이 실행할 수 없다. CLI 스크립트를 Node로 직접 실행한다.
const child = spawn(process.execPath, [
    cli,
    'start',
    ...process.argv.slice(2),
], {
    cwd: desktopDirectory,
    stdio: 'inherit',
    env: appEnv,
});

child.on('error', (error) => {
    console.error(`Paddock을 실행할 수 없습니다: ${error.message}`);
    process.exitCode = 1;
});

for (const signal of ['SIGINT', 'SIGTERM']) {
    process.on(signal, () => child.kill(signal));
}
child.on('exit', (code, signal) => {
    process.exitCode = code === null ? (signal ? 1 : 0) : code;
});
