Object.defineProperty(exports, "__esModule", { value: true });
const { ContainerModule, injectable, decorate } = require('@theia/core/shared/inversify');
const { ElectronMenuContribution } = require('@theia/core/lib/electron-browser/menu/electron-menu-contribution');
const { PaddockMenuContribution } = require('./titlebar');
const { TerminalCopyOnSelectionHandler } = require('@theia/terminal/lib/browser/terminal-copy-on-selection-handler');
const { FrontendApplicationContribution } = require('@theia/core/lib/browser/frontend-application-contribution');
const { SecondaryWindowService } = require('@theia/core/lib/browser/window/secondary-window-service');
const { ElectronSecondaryWindowService } = require('@theia/core/lib/electron-browser/window/electron-secondary-window-service');
const { forgetRestoredWidgets, attachDetachedTabDrag } = require('./tab-detach');
const { FileSystemWatcherErrorHandler } = require('@theia/filesystem/lib/browser/filesystem-watcher-error-handler');
const { OS } = require('@theia/core/lib/common/os');

/**
 * 메인 창으로 되돌린 위젯이 보조 창 닫기를 막지 않게 하는 보조 창 서비스.
 * OS 닫기 버튼은 main 프로세스가 `canClose`로 위젯을 먼저 되돌린 뒤 창을 닫는다. 이때 Theia의 위젯 목록에
 * 되돌린 저장 안 한 에디터가 남아 있으면 보조 창의 beforeunload 검사가 닫기를 막아 빈 창이 남는다.
 * 되돌린 위젯을 목록에서 지워 빈 창은 닫히게 하고, 저장 안 한 내용과 표시는 메인 창의 에디터에 그대로 둔다.
 */
class PaddockSecondaryWindowService extends ElectronSecondaryWindowService {
    /**
     * 보조 창의 탭을 메인 창 칸에 놓는 끌기의 처리기. 작업 화면이 `preview`·`cancel`·`drop`을 채운다.
     * 보조 창은 칸을 모르므로 놓을 자리 계산은 메인 창의 탭 줄 끌기와 같은 쪽에 맡긴다.
     */
    tabDock = {};

    windowCreated(
        newWindow,
        widget,
        shell,
    ) {
        super.windowCreated(newWindow, widget, shell);
        const tabId = target => {
            const tab = target.closest?.('.lm-TabBar-tab');
            const bars = newWindow.rootWidget?.dockPanel.tabBars() ?? [];
            const bar = tab && [...bars].find(item => item.contentNode.contains(tab));
            const index = bar && [...bar.contentNode.children].indexOf(tab);
            return bar?.titles[index]?.owner.id;
        };
        attachDetachedTabDrag(newWindow, window, tabId, {
            preview: (id, x, y) => this.tabDock.preview?.(id, x, y),
            cancel: () => this.tabDock.cancel?.(),
            drop: (id, x, y) => this.tabDock.drop?.(id, x, y),
        });
    }

    /**
     * 보조 창에 있는 위젯 하나를 창을 닫지 않고 메인 창으로 되돌린다.
     * 창을 닫는 복귀(`restoreWidgets`)와 같은 순서(되돌림 알림 → 보조 창 소속 해제 → 메인 칸에 추가)를 밟아
     * 프로세스·출력·저장 안 한 편집이 그대로이고, 그 창에 남은 위젯이 없으면 빈 창만 닫는다.
     *
     * @param widget 되돌릴 위젯. 보조 창에 있지 않거나 이미 닫혔으면 아무것도 하지 않는다.
     * @param shell 메인 창의 셸.
     * @param options 메인 칸 위치(`mode`·`ref`). 없으면 Theia 기본 위치에 붙는다.
     * @returns 되돌렸으면 true.
     */
    async restoreWidget(
        widget,
        shell,
        options = {},
    ) {
        const secondaryWindow = widget.secondaryWindow;
        const isRestorable = Boolean(secondaryWindow) && !widget.isDisposed;
        if (isRestorable) {
            this.beforeWidgetRestoreEmitter.fire([widget, secondaryWindow]);
            widget.secondaryWindow = undefined;
            widget.previousArea = undefined;
            await shell.addWidget(widget, { ...options, area: 'main' });
            await shell.activateWidget(widget.id);
            // 되돌린 위젯이 목록에 남으면 창 닫기가 막힌다(forgetRestoredWidgets). 남은 위젯이 없으면 빈 창을 닫는다.
            forgetRestoredWidgets(secondaryWindow.rootWidget);
            if (!secondaryWindow.closed && !(secondaryWindow.rootWidget?.widgets.length > 0)) secondaryWindow.close();
        }
        return isRestorable;
    }

    async restoreWidgets(
        newWindow,
        extractableWidget,
        shell,
    ) {
        const allMovedOrDisposed = await super.restoreWidgets(newWindow, extractableWidget, shell);
        forgetRestoredWidgets(newWindow.rootWidget);
        return allMovedOrDisposed;
    }
}
decorate(injectable(), PaddockSecondaryWindowService);

// 기본 데스크톱 메뉴가 등록된 뒤 제품 제목줄로 바꾼다.
exports.default = new ContainerModule((bind, unbind, isBound, rebind) => {
    rebind(ElectronMenuContribution).to(PaddockMenuContribution).inSingletonScope();
    rebind(SecondaryWindowService).to(PaddockSecondaryWindowService).inSingletonScope();
    // The terminal module may load later; use its initialized singleton after all modules have loaded.
    bind(FrontendApplicationContribution).toDynamicValue(({ container }) => ({
        onStart: () => {
            const handler = container.get(TerminalCopyOnSelectionHandler);
            handler.copy = async text => {
                if (text) window.electronTheiaCore.writeClipboard(text);
            };
            // Windows의 파일 감시는 WSL 폴더(\\wsl.localhost\…)에 걸리지 않아, Git 확장이 저장소를 열 때마다 실패하고
            // Linux 전용 안내("감시 한도를 올리라")가 뜬다. 한도 문제가 아니므로 Windows에서는 알림 대신 콘솔에만 남긴다.
            // WSL 폴더의 변경은 보이는 폴더를 몇 초마다 다시 읽는 쪽(`watchShownFolder`)이 따라간다.
            if (OS.type() === OS.Type.Windows) {
                container.get(FileSystemWatcherErrorHandler).handleError = async () => {
                    console.warn('File watching failed on a WSL folder; the shown folder is re-read periodically instead.');
                };
            }
        },
    }));
});
