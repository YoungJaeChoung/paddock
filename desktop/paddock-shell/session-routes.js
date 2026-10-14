// 앱이 백엔드(터미널 프로세스를 쥔 프로세스)의 상태를 묻고 다루는 HTTP 경로.
// 앱을 닫을 때 열린 터미널 수를 알아 남길지 정하고, 남은 백엔드에 다시 붙을 때 최신 환경 변수를 넘기며, 백엔드를 정상 절차로 끝낸다.
// Electron 보안 토큰이 없는 요청은 다른 경로와 같이 백엔드 앞단에서 거절한다.
const fs = require('node:fs');
const { injectable, inject, decorate } = require('@theia/core/shared/inversify');
const express = require('express');
const { ProcessManager } = require('@theia/process/lib/node/process-manager');
const { TerminalProcess } = require('@theia/process/lib/node/terminal-process');
const { sharedEnvironment, note } = require('./backend-session');
const { terminalHistoryService } = require('./terminal-history-service');
const { gitShimPath } = require('./git-shim-location');

/**
 * `GET /paddock/session` → `{ terminals }` 열려 있는(끝나지 않은) 터미널 수.
 * `POST /paddock/session/environment` 본문 `{ env }` → 허용된 변수만 이 백엔드의 환경에 덮어쓴다. 이후 새로 여는 터미널이 새 값을 받는다.
 *   이미 열려 있는 터미널의 환경은 바뀌지 않는다.
 * `GET /paddock/session/terminals/:id/history` → 저장해 둔 터미널 기록 `{ savedAt, launcher, cwdNow, agent, output }`. 없으면 404.
 *   앱이 터미널 프로세스째 사라진 뒤 그 터미널을 되살릴 때 화면이 읽는다.
 * `DELETE /paddock/session/terminals/:id/history` → 그 기록을 지운다(되살린 뒤·되살리지 않기로 한 뒤). 없어도 204.
 * `GET /paddock/session/git-shim` → `{ path }` WSL 폴더용 git 분배기 실행 파일 경로. Windows이고 파일이 있을 때만 문자열, 아니면 null. 화면이 `git.path` 기본값으로 쓴다.
 * `POST /paddock/session/stop` 본문 `{ reason }`(없으면 `unknown`)을 `backend.log`에 `Stop requested: <reason>`으로 남기고 202를 돌려준 뒤 일반 종료 절차(터미널 정리 포함)로 백엔드를 끝낸다.
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
        app.get('/paddock/session/git-shim', (request, response) => {
            response.json({ path: gitShimPath(process.platform, process.env.THEIA_APP_PROJECT_PATH || process.cwd(), fs.existsSync) });
        });
        app.post('/paddock/session/environment', express.json({ limit: '256kb' }), (request, response) => {
            Object.assign(process.env, sharedEnvironment((request.body || {}).env));
            response.status(204).end();
        });
        app.get('/paddock/session/terminals/:id/history', (request, response) => {
            const record = /^\d+$/.test(request.params.id) ? terminalHistoryService().history.read(request.params.id) : null;
            if (record) {
                response.json(record);
            } else {
                response.status(404).json({ error: 'No saved terminal.' });
            }
        });
        app.delete('/paddock/session/terminals/:id/history', (request, response) => {
            if (/^\d+$/.test(request.params.id)) terminalHistoryService().history.remove(request.params.id);
            response.status(204).end();
        });
        app.post('/paddock/session/stop', express.json({ limit: '4kb' }), (request, response) => {
            const reason = request.body && typeof request.body.reason === 'string' && request.body.reason ? request.body.reason.slice(0, 100) : 'unknown';
            note(process.env.THEIA_CONFIG_DIR, `Stop requested: ${reason}`);
            response.status(202).end();
            // 종료 신호를 받았을 때와 같은 경로를 탄다. Windows에서는 신호를 보내면 정리 없이 끝나므로 이벤트를 직접 낸다.
            setTimeout(() => process.emit('SIGTERM'), 50);
        });
    }
}
decorate(injectable(), PaddockSessionRoutes);
decorate(inject(ProcessManager), PaddockSessionRoutes.prototype, 'processManager');

module.exports = { PaddockSessionRoutes };
