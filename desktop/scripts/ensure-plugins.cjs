// package.json의 theiaPlugins에 적힌 확장 중 plugins/ 폴더에 없는 것이 있으면 내려받는다.
// 앱 실행(start.cjs)과 설치 파일 제작(package.cjs)이 부른다. 코드를 새로 받은 뒤 setup을 다시 안 돌려도 새 확장이 채워지고,
// 확장이 빠진 채 설치 파일이 만들어지는 일(코드 색·이미지 미리보기가 없는 설치본)을 막는다.
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const desktopDirectory = path.join(__dirname, '..');

/** 필요한데 plugins/ 폴더에 없는 확장 이름 목록. */
function missingPlugins() {
    const manifest = JSON.parse(fs.readFileSync(path.join(desktopDirectory, 'package.json'), 'utf8'));
    const pluginsDirectory = path.join(desktopDirectory, manifest.theiaPluginsDir || 'plugins');
    return Object.keys(manifest.theiaPlugins || {}).filter(name => !fs.existsSync(path.join(pluginsDirectory, name)));
}

/**
 * 빠진 확장을 내려받고, 아직 빠진 것이 남으면 그 이름 목록을 돌려준다(전부 있으면 빈 목록).
 * 네트워크가 없으면 내려받기가 실패하므로 호출한 쪽이 계속할지 멈출지 정한다.
 */
function ensurePlugins() {
    let missing = missingPlugins();
    if (missing.length) {
        console.log(`빠진 확장 ${missing.length}개를 내려받습니다: ${missing.slice(0, 5).join(', ')}${missing.length > 5 ? ' …' : ''}`);
        const cli = path.join(desktopDirectory, 'node_modules', '@theia', 'cli', 'bin', 'theia.js');
        spawnSync(process.execPath, [cli, 'download:plugins', '--rate-limit=15', '--parallel=false'], { cwd: desktopDirectory, stdio: 'inherit' });
        missing = missingPlugins();
    }
    return missing;
}

module.exports = { ensurePlugins, missingPlugins };
