// 백엔드 하나가 쓰는 터미널 화면 저장 장치와 터미널 정보 읽기 장치. 서비스 교체(터미널 추적)와 HTTP 경로가 같은 인스턴스를 쓰게 한다.
const fs = require('node:fs');
const os = require('node:os');
const nodePath = require('node:path');
const { TerminalHistory } = require('./terminal-history');
const { createTerminalInspector } = require('./terminal-inspector');
const { readRestoreOnRestart, RESTORE } = require('./restore-preference');
const { note } = require('./backend-session');

let service = null;

/** 설정 폴더. 메인 프로세스가 백엔드를 띄울 때 THEIA_CONFIG_DIR로 넘긴다. */
function configDirectory() {
    return process.env.THEIA_CONFIG_DIR || nodePath.join(os.homedir(), '.paddock', 'config');
}

/**
 * 이 백엔드의 `{ history, inspector }`를 돌려준다. 처음 부를 때 한 번 만들고 오래된 저장분을 정리한다.
 * 되살리기 설정은 저장 때마다 설정 파일에서 읽어, 끄면 바로 저장을 멈추고 남은 저장분을 지운다.
 */
function terminalHistoryService() {
    if (service === null) {
        const inspector = createTerminalInspector();
        const history = new TerminalHistory({
            directory: nodePath.join(configDirectory(), 'terminal-history'),
            inspect: inspector.inspect,
            isEnabled: () => {
                let settings = '';
                try { settings = fs.readFileSync(nodePath.join(configDirectory(), 'settings.json'), 'utf8'); } catch { /* 설정 파일이 아직 없으면 기본값이다. */ }
                return readRestoreOnRestart(settings) !== RESTORE.NEVER;
            },
            onError: error => note(configDirectory(), `Terminal history error: ${error?.message || error}`),
        });
        history.prune();
        service = { history, inspector };
    }
    return service;
}

module.exports = { terminalHistoryService };
