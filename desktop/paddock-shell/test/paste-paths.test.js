const test = require('node:test');
const assert = require('node:assert/strict');
const { PASTE_PLACE, toWslPath, pastePathText, imageExtension } = require('../paste-paths');

test('Windows 경로를 WSL 셸의 경로로 바꾼다', () => {
    assert.equal(toWslPath('C:\\Users\\me\\shot.png'), '/mnt/c/Users/me/shot.png');
    assert.equal(toWslPath('D:\\'), '/mnt/d');
    assert.equal(toWslPath('\\\\wsl.localhost\\Ubuntu\\home\\me\\a.png'), '/home/me/a.png');
    assert.equal(toWslPath('\\\\wsl$\\Ubuntu\\tmp'), '/tmp');
    assert.equal(toWslPath('\\\\server\\share\\a.png'), '\\\\server\\share\\a.png');
});

test('터미널 종류에 맞게 경로를 감싸 빈칸으로 잇는다', () => {
    assert.equal(pastePathText(['C:\\My Files\\a.png', 'C:\\b.png'], PASTE_PLACE.WSL), '/mnt/c/My\\ Files/a.png /mnt/c/b.png');
    assert.equal(pastePathText(['C:\\My Files\\a.png'], PASTE_PLACE.WINDOWS), '"C:\\My Files\\a.png"');
    assert.equal(pastePathText(['C:\\a.png'], PASTE_PLACE.WINDOWS), 'C:\\a.png');
    assert.equal(pastePathText(["/Users/me/it's (1).png"], PASTE_PLACE.POSIX), "/Users/me/it\\'s\\ \\(1\\).png");
    assert.equal(pastePathText(['/home/me/한글.png'], PASTE_PLACE.POSIX), '/home/me/한글.png');
    assert.equal(pastePathText([], PASTE_PLACE.POSIX), '');
});

test('이미지 형식에 맞는 확장자를 고른다', () => {
    assert.equal(imageExtension('image/jpeg'), 'jpg');
    assert.equal(imageExtension('image/png'), 'png');
    assert.equal(imageExtension(''), 'png');
});
