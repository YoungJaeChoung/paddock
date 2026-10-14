// 백엔드가 이 화면의 연결을 버려 재접속에 실패하면 화면을 새로 고친다. 저장하지 않은 편집 내용이 있으면 사용자가 정하게 알린다.
const { Saveable } = require('@theia/core/lib/browser/saveable');
const { WindowService } = require('@theia/core/lib/browser/window/window-service');
const { WebSocketConnectionSource } = require('@theia/core/lib/browser/messaging/ws-connection-source');
const { ConnectionManagementMessages } = require('@theia/core/lib/common/messaging/connection-management');
const { MessageService } = require('@theia/core/lib/common/message-service');
const { EditorManager } = require('@theia/editor/lib/browser/editor-manager');

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
 * 저장하지 않은 편집기가 있으면 자동으로 새로 고치지 않고 알림만 띄운다. 끊긴 동안 쓴 내용은 저장도 못 한 상태라 새로 고치면 사라진다.
 * 알림의 "Reload window"는 사용자가 복사해 둔 뒤 누르는 새로 고침이며 Theia의 저장 확인을 거친다.
 */
class PaddockReconnectGuard {
    constructor(
        container,
    ) {
        this.container = container;
    }

    onStart() {
        this.container.get(WebSocketConnectionSource).socket.on(ConnectionManagementMessages.RECONNECT, (hasConnection) => {
            if (hasConnection === false) this.handleReconnectFailed();
        });
    }

    handleReconnectFailed() {
        const n_dirty = this.container.get(EditorManager).all.filter(editor => Saveable.isDirty(editor)).length;
        if (n_dirty > 0) void this.notify(n_dirty);
        else this.container.get(WindowService).reload();
    }

    async notify(
        n_dirty,
    ) {
        const reload = 'Reload window';
        const answer = await this.container.get(MessageService).warn(
            `Lost the connection to Paddock's background service. ${n_dirty === 1 ? '1 editor has' : `${n_dirty} editors have`} unsaved changes that could not be saved. Copy what you need, then reload.`,
            reload,
        );
        if (answer === reload) this.container.get(WindowService).reload();
    }
}

module.exports = { PaddockReconnectGuard };
