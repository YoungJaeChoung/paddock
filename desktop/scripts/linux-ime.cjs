// Linux(WSL 포함)에서 앱 창에 한글을 입력할 수 있게 입력기(ibus)를 준비한다.
// WSL의 Linux 앱 창은 Windows 한글 입력기를 받지 못하므로 Linux 쪽 입력기가 떠 있어야 한다.
// 앱(Electron, X11)은 GTK 입력기 모듈로 ibus에 붙는데, 데몬이 꺼져 있으면 영문만 들어간다.
const { spawnSync } = require('node:child_process');

function run(
    command,
    args,
) {
    const result = spawnSync(command, args, { encoding: 'utf8' });
    return { ok: !result.error && result.status === 0, stdout: result.stdout || '' };
}

/**
 * 입력기 준비 결과를 돌려준다. 앱 실행 환경에 넣을 변수(`env`)와 사용자에게 보일 한 줄(`message`)이다.
 * Linux가 아니면 아무것도 하지 않는다. ibus가 없으면 설치 안내를, 한글 엔진이 없으면 엔진 설치 안내를 준다.
 */
function prepareLinuxIme() {
    let prepared = { env: {}, message: null };
    if (process.platform === 'linux') {
        if (!run('which', ['ibus-daemon']).ok) {
            prepared.message = '한글 입력: ibus가 없어 영문만 입력됩니다. sudo apt install ibus ibus-hangul 후 다시 실행하세요.';
        } else {
            // 다른 입력기(fcitx 등)를 이미 쓰는 환경이면 건드리지 않는다.
            const im = process.env.GTK_IM_MODULE;
            if (!im || im === 'ibus') {
                prepared.env = { GTK_IM_MODULE: 'ibus', QT_IM_MODULE: 'ibus', XMODIFIERS: '@im=ibus' };
                if (!run('ibus', ['engine']).ok) {
                    // -d 백그라운드, -r 이미 있으면 교체, -x XIM 서버도 켠다.
                    run('ibus-daemon', ['-drx']);
                    for (let n_tries = 0; n_tries < 30 && !run('ibus', ['engine']).ok; n_tries += 1) {
                        spawnSync('sleep', ['0.1']);
                    }
                }
                const hasHangul = run('ibus', ['list-engine']).stdout.includes('hangul');
                if (!hasHangul) {
                    prepared.message = '한글 입력: 한글 엔진이 없습니다. sudo apt install ibus-hangul 후 다시 실행하세요.';
                } else {
                    run('ibus', ['engine', 'hangul']);
                    prepared.message = '한글 입력: ibus 한글 켜짐 — 한/영 전환은 Hangul 키 또는 Ctrl+Space.';
                }
            }
        }
    }
    return prepared;
}

module.exports = { prepareLinuxIme };
