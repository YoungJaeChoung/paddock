// 화면(렌더러)이 죽은 뒤 백엔드에 남은 그 화면의 옛 연결을 앱이 닫게 하는 HTTP 경로.
// Electron 보안 토큰이 없는 요청은 다른 경로와 같이 백엔드 앞단에서 거절한다.
const { injectable, inject, decorate } = require('@theia/core/shared/inversify');
const { WebsocketFrontendConnectionService } = require('@theia/core/lib/node/messaging/websocket-frontend-connection-service');
const { note } = require('./backend-session');

/**
 * 연결이 끊긴 채 연결 유지 시간을 기다리는 화면 연결을 모두 닫고, 닫은 수를 돌려준다.
 *
 * 화면이 죽으면 그 화면의 소켓이 끊기고 백엔드는 같은 화면이 다시 붙기를 기다리며 연결(확장 호스트 포함)을 유지한다.
 * 죽은 화면은 다시 붙지 않고 새 화면이 새 연결을 맺으므로, 기다리는 동안 확장 호스트가 둘이 되어 같은 저장소에
 * git 명령이 겹친다. 앱이 죽은 화면을 다시 불러오기 직전에 이 경로를 불러 옛 연결을 바로 닫는다.
 * 다른 창이 잠시 끊겨 기다리는 중이라면 그 연결도 닫히지만, 그 화면은 재접속 실패로 새로 고침된다(reconnect-guard.js).
 *
 * Examples
 * --------
 * | 기다리는 연결 | 반환 |
 * |---------------|------|
 * | 없음          | 0    |
 * | `1`, `2`      | 2    |
 */
function closeDisconnectedFrontends(
    service,
) {
    const waiting = [...service.closeTimeouts.keys()];
    for (const frontendId of waiting) service.closeConnection(frontendId, 'renderer-gone');
    return waiting.length;
}

/** `POST /paddock/session/close-disconnected-frontends` → `{ closed }` 닫은 연결 수. */
class PaddockFrontendRoutes {
    configure(
        app,
    ) {
        app.post('/paddock/session/close-disconnected-frontends', (request, response) => {
            const n_closed = closeDisconnectedFrontends(this.connectionService);
            note(process.env.THEIA_CONFIG_DIR, `closed ${n_closed} disconnected frontend connection(s) after a renderer died`);
            response.json({ closed: n_closed });
        });
    }
}
decorate(injectable(), PaddockFrontendRoutes);
decorate(inject(WebsocketFrontendConnectionService), PaddockFrontendRoutes.prototype, 'connectionService');

module.exports = { PaddockFrontendRoutes, closeDisconnectedFrontends };
