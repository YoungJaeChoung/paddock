// 창·프로세스 사건을 backend.log에 남기고, 화면(렌더러)이 죽으면 창을 다시 불러 복구한다.
// 화면이 죽거나 창이 닫혀도 예전에는 어떤 기록도 남지 않아 "앱이 갑자기 닫혔다"는 보고의 원인을 가릴 수 없었다.
const { app, dialog } = require('electron');

/** 화면 자동 복구 규칙. 5분 안에 세 번까지 다시 불러오고, 그보다 잦으면 반복 충돌로 보고 사용자에게 묻는다. */
class RECOVERY {
    static WINDOW_MS = 5 * 60 * 1000;
    static LIMIT = 3;
    /** 사용자나 OS가 일부러 끝낸 경우. 되살리면 오히려 끝내려는 뜻을 거스른다. */
    static INTENDED_EXIT_REASONS = ['clean-exit', 'killed'];
}

/**
 * 화면·하위 프로세스가 사라진 사건을 backend.log에 한 줄로 쓸 문장으로 바꾼다.
 *
 * `render-process-gone`의 세부 정보는 `reason`·`exitCode`만 있고, `child-process-gone`은 `type`(GPU·Utility 등)과
 * 선택적으로 `name`이 더 있다. `type`이 있으면 하위 프로세스, 없으면 화면으로 읽는다.
 *
 * Examples
 * --------
 * | details                                              | 문장                                       |
 * |------------------------------------------------------|--------------------------------------------|
 * | `{ reason: 'crashed', exitCode: -1073741819 }`       | `renderer gone: crashed (exitCode=-1073741819)` |
 * | `{ type: 'GPU', reason: 'launch-failed', exitCode: 1 }` | `GPU process gone: launch-failed (exitCode=1)` |
 * | `{ type: 'Utility', name: 'network', reason: 'killed', exitCode: 9 }` | `Utility process (network) gone: killed (exitCode=9)` |
 */
function describeGone(
    details,
) {
    const subject = details.type ? `${details.type} process${details.name ? ` (${details.name})` : ''}` : 'renderer';
    return `${subject} gone: ${details.reason} (exitCode=${details.exitCode})`;
}

/**
 * 화면이 죽었을 때 자동으로 다시 불러올지 정하고, 이번 사건을 포함한 최근 기록을 돌려준다.
 *
 * 다시 불러오는 횟수를 세어 두는 이유는 시작하자마자 죽는 화면을 무한히 다시 불러 CPU와 로그를 태우지 않기 위해서다.
 * `history`는 이전 사건 시각(ms) 목록이며, 윈도 밖의 오래된 것은 버린다. 이번 사건 시각은 다시 불러오든 묻든 항상 기록에 더한다.
 * 윈도 안 이전 사건이 `limit`회 미만이면 다시 불러온다.
 *
 * Examples
 * --------
 * | 윈도 안 이전 사건 수 | limit | isReload |
 * |----------------------|-------|----------|
 * | 0                    | 3     | true     |
 * | 2                    | 3     | true     |
 * | 3                    | 3     | false    |
 * | 3 (모두 윈도 밖)     | 3     | true     |
 */
function shouldReload(
    history,
    now,
    limit = RECOVERY.LIMIT,
    windowMs = RECOVERY.WINDOW_MS,
) {
    const recent = history.filter(time => now - time < windowMs);
    return { isReload: recent.length < limit, history: [...recent, now] };
}

/**
 * 창 하나의 화면 사망·응답 없음·닫기 요청·닫힘을 backend.log에 남기고, 화면이 죽으면 복구한다.
 *
 * 복구는 `clean-exit`·`killed`가 아닌 사망에서만 한다. 화면만 다시 불러오면 백엔드와 터미널은 그대로 살아 있고,
 * 새 화면이 저장된 배치(layout-autosave.js)로 터미널 탭에 다시 접속한다. 5분 안에 세 번을 넘어 죽으면 다시 불러오지 않고
 * 대화상자로 다시 불러올지 창을 닫을지 묻는다. 창을 닫으면 일반 종료 경로(will-quit)를 따른다.
 * `describeState`는 창 닫기 요청 때 앱 상태(init·ready 등)를 문장에 넣으려고 부르며, 모르면 빈 문자열을 돌려도 된다.
 */
function attachWindowEventLog(
    window,
    note,
    describeState = () => '',
) {
    let history = [];
    const { webContents } = window;
    webContents.on('render-process-gone', (event, details) => {
        note(describeGone(details));
        if (!RECOVERY.INTENDED_EXIT_REASONS.includes(details.reason) && !window.isDestroyed()) {
            const decision = shouldReload(history, Date.now());
            history = decision.history;
            if (decision.isReload) {
                note(`reloading the window (${decision.history.length} of ${RECOVERY.LIMIT} automatic reloads in ${RECOVERY.WINDOW_MS / 60000} min)`);
                webContents.reload();
            } else {
                note(`renderer keeps dying (more than ${RECOVERY.LIMIT} in ${RECOVERY.WINDOW_MS / 60000} min); asking the user`);
                askReload(window, note);
            }
        }
    });
    window.on('unresponsive', () => note('window unresponsive'));
    window.on('responsive', () => note('window responsive again'));
    window.on('close', () => note(`window close requested${describeState() ? ` (${describeState()})` : ''}`));
    window.on('closed', () => note('window closed'));
}

/** 반복해서 죽는 창을 다시 불러올지 닫을지 묻는다. 다시 불러오기를 고르면 그 창의 기록은 그대로 두어 다음 사망에도 다시 묻는다. */
async function askReload(
    window,
    note,
) {
    const { response } = await dialog.showMessageBox(window, {
        type: 'error',
        buttons: ['Reload', 'Close window'],
        defaultId: 0,
        cancelId: 1,
        title: 'Paddock',
        message: 'The Paddock window stopped working again.',
        detail: `It was reloaded ${RECOVERY.LIMIT} times in the last ${RECOVERY.WINDOW_MS / 60000} minutes. Your terminals keep running in the background. Reload to reconnect, or close the window.`,
    });
    note(`recovery dialog answered: ${response === 0 ? 'reload' : 'close'}`);
    if (!window.isDestroyed()) {
        if (response === 0) window.webContents.reload();
        else window.destroy();
    }
}

/**
 * 앱 전체 수준 사건(하위 프로세스 사망·잡히지 않은 예외·종료 시작과 확정)을 backend.log에 남긴다.
 *
 * `uncaughtException`에 직접 리스너를 달면 Electron 기본 동작(오류 대화상자·종료 처리)이 사라지므로,
 * 동작을 바꾸지 않고 알려만 주는 `uncaughtExceptionMonitor`를 쓴다.
 */
function attachAppEventLog(
    note,
) {
    app.on('child-process-gone', (event, details) => note(describeGone(details)));
    process.on('uncaughtExceptionMonitor', (error, origin) => note(`uncaught exception (${origin}): ${error && error.stack ? error.stack : error}`));
    app.on('before-quit', () => note('before-quit'));
    app.on('will-quit', () => note('will-quit'));
}

module.exports = { RECOVERY, describeGone, shouldReload, attachWindowEventLog, attachAppEventLog };
