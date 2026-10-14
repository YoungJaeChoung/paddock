// 화면 배치(어떤 터미널 탭이 어디 있는지)를 작업 중에도 주기적으로 저장한다.
// Theia는 창이 정상으로 닫힐 때만 배치를 저장해, 앱이 강제로 끝나면(설치 프로그램의 갱신 종료·충돌) 마지막 정상 종료 뒤에 연 터미널이 배치에서 사라진다.
// 저장한 배치가 있어야 다시 열 때 그 터미널 탭이 되살아나고, 탭마다 저장해 둔 화면·폴더를 이어 받는다(terminal.js의 되살리기).
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { ShellLayoutRestorer } = require('@theia/core/lib/browser/shell/shell-layout-restorer');
const { injectable, inject, decorate } = require('@theia/core/shared/inversify');
const { TrailingDebounce } = require('./trailing-debounce');
const { WINDOW_ROLE, windowRole } = require('./window-role');
const { PaddockTerminal } = require('./terminal');
const { PaddockWorkspace } = require('./workspace');
const { STORAGE } = require('./workspace/shared');

class LayoutAutosave {
    /** 배치가 바뀐 뒤 이만큼 조용하면 저장한다(ms). */
    static QUIET_MS = 5000;
    /** 배치가 계속 바뀌어도 이 시간 안에는 저장한다(ms). */
    static MAX_WAIT_MS = 15000;
}

/**
 * 위젯 추가·제거·활성 변경·탭 이동 뒤 5초 조용하면 배치를 저장한다(계속 바뀌어도 15초 안에 한 번).
 * 지난 배치를 되살리는 일이 끝난 뒤에 시작한다 — 되살리는 중에 저장하면 아직 덜 되살린 배치가 저장된 배치를 덮는다.
 * 보조 창은 저장소를 메모리로 쓰므로(window-role.js) 저장하지 않는다.
 */
class PaddockLayoutAutosave {
    onDidInitializeLayout() {
        if (windowRole(window.location.search) === WINDOW_ROLE.PRIMARY) {
            this.debounce = new TrailingDebounce({
                quietMs: LayoutAutosave.QUIET_MS, maxWaitMs: LayoutAutosave.MAX_WAIT_MS, run: () => this.store(),
            });
            const schedule = () => this.debounce.touch();
            this.shell.onDidAddWidget(schedule);
            this.shell.onDidRemoveWidget(schedule);
            this.shell.onDidChangeActiveWidget(schedule);
            this.shell.mainPanel.layoutModified.connect(schedule);
        }
    }

    /**
     * 지금 배치를 저장한다. 창을 닫을 때와 같은 배치를 만들되 화면은 바꾸지 않는다 — 닫을 때는 다른 묶음의 위젯을 본문에 도로 모으지만(workspace.js의 onWillStop),
     * 작업 중에 그렇게 하면 화면이 바뀌므로 합친 배치만 계산해 저장하는 쪽에 넘긴다.
     */
    store() {
        // 작업 목록을 되살리는 중이거나 묶음을 바꾸는 중에는 본문 배치가 중간 상태라 저장하지 않고 다음 주기로 미룬다.
        if (!this.workspace.layoutReady || this.workspace.isSwitchingRoot) {
            this.debounce.touch();
        } else {
            this.storeNow();
        }
    }

    storeNow() {
        // 터미널이 "창을 닫는 중"으로 표시하지 않게 알린다(PaddockTerminal.isStoringAutomatically).
        PaddockTerminal.isStoringAutomatically = true;
        try {
            const { layouts, merged } = this.workspace.mergedRootLayouts();
            const data = this.shell.getLayoutData();
            if (merged) data.mainPanel = { ...data.mainPanel, main: merged };
            this.workspace.storeRootLayouts(layouts);
            this.workspace.storage.setData(STORAGE.SHOWN_WIDGET, this.workspace.currentWidget()?.id ?? '');
            // 창이 닫힐 때 Theia가 하는 저장 호출(frontend-application.js의 windowsService.onUnload)에 합친 배치를 넘긴다.
            this.layoutRestorer.storeLayout({ shell: { getLayoutData: () => data } });
        } finally {
            PaddockTerminal.isStoringAutomatically = false;
        }
    }
}
decorate(injectable(), PaddockLayoutAutosave);
decorate(inject(ApplicationShell), PaddockLayoutAutosave.prototype, 'shell');
decorate(inject(ShellLayoutRestorer), PaddockLayoutAutosave.prototype, 'layoutRestorer');
decorate(inject(PaddockWorkspace), PaddockLayoutAutosave.prototype, 'workspace');

module.exports = { PaddockLayoutAutosave, LayoutAutosave };
