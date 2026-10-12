// 앱이 백엔드(터미널 프로세스를 쥔 프로세스)의 상태를 묻고 다루는 HTTP 경로.
// 앱을 닫을 때 열린 터미널 수를 알아 남길지 정하고, 남은 백엔드에 다시 붙을 때 최신 환경 변수를 넘기며, 백엔드를 정상 절차로 끝낸다.
// Electron 보안 토큰이 없는 요청은 다른 경로와 같이 백엔드 앞단에서 거절한다.
const { injectable, inject, decorate } = require('@theia/core/shared/inversify');
const express = require('express');
const { ProcessManager } = require('@theia/process/lib/node/process-manager');
const { TerminalProcess } = require('@theia/process/lib/node/terminal-process');
const { sharedEnvironment } = require('./backend-session');

/**
 * `GET /paddock/session` → `{ terminals }` 열려 있는(끝나지 않은) 터미널 수.
 * `POST /paddock/session/environment` 본문 `{ env }` → 허용된 변수만 이 백엔드의 환경에 덮어쓴다. 이후 새로 여는 터미널이 새 값을 받는다.
 *   이미 열려 있는 터미널의 환경은 바뀌지 않는다.
 * `POST /paddock/session/stop` → 202를 돌려준 뒤 일반 종료 절차(터미널 정리 포함)로 백엔드를 끝낸다.
 */
class PaddockSessionRoutes {
    configure(
        app,
    ) {
        app.get('/paddock/session', (request, response) => {
            let n_terminals = 0;
            for (const managed of this.processManager.processes.values()) {
                if (managed instanceof TerminalProcess && !managed.killed) n_terminals += 1;
            }
            response.json({ terminals: n_terminals });
        });
        app.post('/paddock/session/environment', express.json({ limit: '256kb' }), (request, response) => {
            Object.assign(process.env, sharedEnvironment((request.body || {}).env));
            response.status(204).end();
        });
        app.post('/paddock/session/stop', (request, response) => {
            response.status(202).end();
            // 종료 신호를 받았을 때와 같은 경로를 탄다. Windows에서는 신호를 보내면 정리 없이 끝나므로 이벤트를 직접 낸다.
            setTimeout(() => process.emit('SIGTERM'), 50);
        });
    }
}
decorate(injectable(), PaddockSessionRoutes);
decorate(inject(ProcessManager), PaddockSessionRoutes.prototype, 'processManager');

module.exports = { PaddockSessionRoutes };
