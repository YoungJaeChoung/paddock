// 백엔드가 이 화면의 연결을 버려 재접속에 실패하면 화면을 새로 고친다. 저장하지 않은 편집 내용이 있으면 사용자가 정하게 알린다.
const { Saveable } = require('@theia/core/lib/browser/saveable');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { WindowService } = require('@theia/core/lib/browser/window/window-service');
const { WebSocketConnectionSource } = require('@theia/core/lib/browser/messaging/ws-connection-source');
const { ConnectionManagementMessages } = require('@theia/core/lib/common/messaging/connection-management');
const { MessageService } = require('@theia/core/lib/common/message-service');
const { EditorManager } = require('@theia/editor/lib/browser/editor-manager');
const { dirtyWidgets, reconnectAction, lostConnectionMessage } = require('./reconnect-decision');

/** 알림을 사용자가 닫기만 하고 새로 고치지 않았을 때 다시 띄우기까지의 시간(ms). */
const RENOTIFY_MS = 30 * 1000;

/**
 * 연결 복구 실패를 듣고 화면을 새로 고친다. 설정 `reloadOnReconnect: true`와 짝으로 쓴다.
 *
 * 백엔드는 화면이 끊긴 채 `frontendConnectionTimeout`(30초)이 지나면 그 연결을 버린다. 그 뒤 재접속은 실패하고,
 * 설정이 꺼져 있으면 Theia가 같은 화면에서 새 연결을 맺는다. 그러면 화면에 남은 확장 호스트 통신과 새로 뜬 호스트의 통신이
 * 엇갈려 같은 오류가 수만 줄 쌓이고 메모리가 불어난다. 그래서 설정을 켜 그 제자리 재접속을 막는다.
 * 켜 두면 Theia는 `location.reload()`를 부르지만 Electron 메인이 화면 이동을 막아(will-navigate) 아무 일도 일어나지 않으므로,
 * 새로 고침은 여기서 Theia의 일반 새로 고침(`WindowService.reload`)으로 직접 한다.
 * 이 새로 고침은 터미널을 닫지 않고(백엔드에 살아 있어 다시 접속한다) 종료 질문도 띄우지 않는다.
 *
 * 저장하지 않은 편집기가 있으면 자동으로 새로 고치지 않고 알림을 띄운다. 끊긴 동안 쓴 내용은 저장도 못 한 상태라
 * 새로 고치면 사라진다. 재접속 시도는 한 번뿐이라(Theia가 재접속 때 쓰는 이벤트 처리기를 한 번 쓰고 지운다) 알림을 닫아도
 * 복구할 다른 길이 없으므로, 새로 고치기 전까지 알림을 `RENOTIFY_MS`마다 다시 띄운다. 알림의 "Reload window"는
 * 내용을 복사해 둔 뒤 누르는 새로 고침이며 Theia의 저장 확인을 거친다. 다시 띄울 때 저장하지 않은 편집기가 사라졌으면 바로 새로 고친다.
 */
class PaddockReconnectGuard {
    constructor(
        container,
    ) {
        this.container = container;
    }

    onStart() {
        this.container.get(WebSocketConnectionSource).socket.on(ConnectionManagementMessages.RECONNECT, (hasConnection) => {
            if (hasConnection === false) void this.handleReconnectFailed();
        });
    }

    /** Theia의 저장 확인이 보는 위젯과 같은 범위(메인 영역·편집기 관리자·보조 창)에서 저장하지 않은 위젯을 센다. */
    countDirty() {
        const shell = this.container.get(ApplicationShell);
        const secondaryWidgets = (shell.secondaryWindowHandler?.secondaryWindowService?.secondaryWindows ?? []).flatMap(secondary => secondary.rootWidget?.widgets ?? []);
        return dirtyWidgets([shell.widgets, this.container.get(EditorManager).all, secondaryWidgets], widget => Saveable.isDirty(widget)).length;
    }

    /** 미저장 편집기가 없으면 새로 고치고, 있으면 사용자가 새로 고치거나 편집기가 비기까지 알림을 되풀이한다. */
    async handleReconnectFailed() {
        let isDone = false;
        while (!isDone) {
            const n_dirty = this.countDirty();
            if (reconnectAction(n_dirty) === 'reload') {
                this.reload();
                isDone = true;
            } else {
                const reload = 'Reload window';
                const answer = await this.container.get(MessageService).warn(lostConnectionMessage(n_dirty), reload);
                if (answer === reload) {
                    this.reload();
                    isDone = true;
                } else {
                    await new Promise(resolve => setTimeout(resolve, RENOTIFY_MS));
                }
            }
        }
    }

    reload() {
        this.container.get(WindowService).reload();
    }
}

module.exports = { PaddockReconnectGuard };
