// 이름을 Herdr Terminal에서 Paddock으로 바꾸기 전에 쓰던 저장 위치를 새 위치로 옮긴다.
const fs = require('node:fs');
const path = require('node:path');

// 옛 이름이 남아 있는 곳. 폴더 이름, 사용자 설정 키의 접두사, Claude Code 설정 백업 파일의 꼬리.
const LEGACY = {
    directoryName: '.herdr',
    preferencePrefix: '"herdr.',
    backupSuffix: '.herdr-backup',
};

/** 파일 내용의 글자를 바꾸고, 바뀐 것이 있을 때만 다시 쓴다. */
function replaceInFile(
    filePath,
    from,
    to,
) {
    if (fs.existsSync(filePath)) {
        const text = fs.readFileSync(filePath, 'utf8');
        const next = text.replaceAll(from, to);
        if (next !== text) fs.writeFileSync(filePath, next);
    }
}

/**
 * 옛 앱 폴더(`~/.herdr`)를 새 앱 폴더로 옮기고, 옛 경로·이름을 가리키던 설정을 고친다.
 *
 * 이름을 바꾼 뒤에도 설치한 확장·사용자 설정·AI 사용량 기록을 그대로 이어 쓰게 한다.
 * Claude Code 설정의 상태 줄 명령은 앱 폴더 안의 스크립트를 절대 경로로 부르므로, 폴더만 옮기면
 * Claude Code 상태 줄이 깨진다. 그래서 그 경로도 함께 바꾼다.
 *
 * Parameters
 * ----------
 * homeDirectory : string
 *     옛 폴더가 있는 홈 폴더.
 * directory : string
 *     새 앱 폴더(예 `~/.paddock`).
 * claudeSettingsPath : string
 *     Claude Code 설정 파일 경로.
 *
 * Returns
 * -------
 * boolean
 *     옮겼으면 true. 옛 폴더가 없거나 새 폴더가 이미 있으면 아무것도 하지 않고 false.
 */
function moveLegacyDirectory(
    homeDirectory,
    directory,
    claudeSettingsPath,
) {
    const legacyDirectory = path.join(homeDirectory, LEGACY.directoryName);
    const moved = fs.existsSync(legacyDirectory) && !fs.existsSync(directory);
    if (moved) {
        fs.renameSync(legacyDirectory, directory);
        // 사용자 설정은 주석이 든 JSON일 수 있어 해석하지 않고 키의 글자만 바꾼다.
        replaceInFile(path.join(directory, 'config', 'settings.json'), LEGACY.preferencePrefix, '"paddock.');
        // JSON 안의 경로는 역슬래시가 이스케이프되어 있으므로(Windows) 같은 방식으로 적은 글자를 찾는다.
        // 경로 구분자까지 붙여 `.herdr`로 시작하는 다른 폴더 이름은 건드리지 않는다.
        const escapedPath = (value) => JSON.stringify(value + path.sep).slice(1, -1);
        replaceInFile(claudeSettingsPath, escapedPath(legacyDirectory), escapedPath(directory));
        const legacyBackup = `${claudeSettingsPath}${LEGACY.backupSuffix}`;
        const backup = `${claudeSettingsPath}.paddock-backup`;
        if (fs.existsSync(legacyBackup) && !fs.existsSync(backup)) fs.renameSync(legacyBackup, backup);
    }
    return moved;
}

module.exports = { moveLegacyDirectory };
