const { EnvVariablesServer } = require('@theia/core/lib/common/env-variables');
const { FileService } = require('@theia/filesystem/lib/browser/file-service');
const { URI } = require('@theia/core/lib/common/uri');
const { STOP_ONCE_FILE } = require('./backend-session');

/** 창 닫기가 취소됐는지 보고 표지를 거두기까지 기다리는 시간(ms). */
const CANCEL_CLEANUP_MS = 30000;

/**
 * 설정과 무관하게 이번 종료에서만 남은 터미널을 모두 끝내는 명령 "Paddock: Quit and Stop All Terminals".
 *
 * 종료 설정을 "계속 실행(keep)"으로 둔 사용자가 백엔드를 한 번 끄고 싶을 때 쓴다. 메인 프로세스는 종료를 확정할 때
 * 설정 폴더의 표지 파일을 읽고 지우므로, 이 명령은 표지를 적고 창을 닫는다. 닫기가 취소되면 표지를 거둬 다음 종료에 영향이 없게 한다.
 */
class PaddockBackendStop {
    constructor(
        container,
    ) {
        this.container = container;
    }

    registerCommands(
        registry,
    ) {
        registry.registerCommand(
            { id: 'paddock.quitAndStopTerminals', label: 'Paddock: Quit and Stop All Terminals' },
            { execute: () => this.quitAndStop() },
        );
    }

    async quitAndStop() {
        const directory = new URI(await this.container.get(EnvVariablesServer).getConfigDirUri());
        const flag = directory.resolve(STOP_ONCE_FILE);
        const files = this.container.get(FileService);
        await files.write(flag, String(Date.now()));
        window.electronTheiaCore.close();
        // 창이 닫히면 이 화면과 타이머가 함께 사라지고 메인이 표지를 쓴다. 저장 확인 등으로 닫기가 취소돼 아직 살아 있으면 표지를 거둔다.
        setTimeout(() => files.delete(flag).catch(() => undefined), CANCEL_CLEANUP_MS);
    }
}

module.exports = { PaddockBackendStop };
