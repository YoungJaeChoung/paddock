// 터미널에 붙여 넣을 파일 경로를 그 터미널의 셸이 읽는 형식으로 바꾸는 순수 함수. DOM·Theia에 기대지 않아 Node 테스트로 바로 검증한다.
// 탐색기에서 복사한 파일이나 저장한 클립보드 이미지는 앱이 도는 운영체제의 경로라, WSL 셸에는 /mnt/<드라이브> 경로로 바꿔 넘긴다.

const PASTE_PLACE = {
    WSL: 'wsl',
    WINDOWS: 'windows',
    POSIX: 'posix',
};

/** Windows 경로를 WSL 셸에서 같은 파일을 가리키는 경로로 바꾼다. 바꿀 수 없는 형식은 그대로 둔다. */
function toWslPath(
    windowsPath,
) {
    let result = windowsPath;
    const drive = /^([A-Za-z]):[\\/]?(.*)$/.exec(windowsPath);
    // \\wsl.localhost\<배포판>\... 과 \\wsl$\<배포판>\... 은 배포판 안의 경로다.
    const distribution = /^\\\\wsl(?:\.localhost|\$)\\[^\\]+(\\.*)?$/i.exec(windowsPath);
    if (drive) result = `/mnt/${drive[1].toLowerCase()}/${drive[2].replace(/\\/g, '/')}`.replace(/\/$/, '');
    else if (distribution) result = (distribution[1] || '\\').replace(/\\/g, '/');
    return result;
}

/** 셸이 한 단어로 읽도록 경로를 감싼다. POSIX 셸은 macOS 터미널의 끌어 놓기처럼 특수 문자 앞에 \를 붙인다. */
function quotePath(
    filePath,
    place,
) {
    let result = filePath;
    if (place === PASTE_PLACE.WINDOWS) {
        if (/[\s&()^,;=!'%]/.test(filePath)) result = `"${filePath}"`;
    } else {
        result = filePath.replace(/([^\p{L}\p{N}_@%+=:,./~-])/gu, '\\$1');
    }
    return result;
}

/**
 * 붙여 넣을 파일 경로 목록을 터미널 입력 한 줄로 만든다.
 *
 * 경로마다 터미널 종류에 맞게 바꾸고 감싼 뒤 빈칸으로 잇는다. 경로가 없으면 빈 문자열이다.
 *
 * Examples
 * --------
 * | 경로                         | 터미널  | 결과                          |
 * | ---------------------------- | ------- | ----------------------------- |
 * | `C:\Users\me\shot.png`       | wsl     | `/mnt/c/Users/me/shot.png`    |
 * | `C:\My Files\a.png`          | wsl     | `/mnt/c/My\ Files/a.png`      |
 * | `C:\My Files\a.png`          | windows | `"C:\My Files\a.png"`         |
 * | `/Users/me/My Shot.png`      | posix   | `/Users/me/My\ Shot.png`      |
 */
function pastePathText(
    paths,
    place,
) {
    return paths
        .map(filePath => quotePath(place === PASTE_PLACE.WSL ? toWslPath(filePath) : filePath, place))
        .join(' ');
}

/** 클립보드 이미지의 형식에 맞는 확장자. 모르는 형식은 png로 저장한다(브라우저가 클립보드 그림을 png로 넘긴다). */
function imageExtension(
    mimeType,
) {
    const extensions = { 'image/jpeg': 'jpg', 'image/gif': 'gif', 'image/webp': 'webp', 'image/bmp': 'bmp' };
    return extensions[mimeType] || 'png';
}

module.exports = { PASTE_PLACE, toWslPath, pastePathText, imageExtension };
