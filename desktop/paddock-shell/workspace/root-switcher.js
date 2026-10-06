const { WebviewWidget } = require('@theia/plugin-ext/lib/main/browser/webview/webview');
const { PreferencesWidget } = require('@theia/preferences/lib/browser/views/preference-widget');
const layoutModel = require('../layout-model');
const { STORAGE } = require('./shared');

/**
 * 묶음(작업 폴더의 맨 위 터미널)마다 본문 칸 배치를 기억하고, 다른 묶음을 고르면 그 묶음의 배치로 본문을 갈아 끼운다.
 */
class RootSwitcher {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
    }

    /**
     * 위젯이 속한 묶음 이름. 작업 폴더의 터미널·파일은 그 폴더, 위쪽 터미널과 그 내부 터미널·파일은 위쪽 터미널이다.
     * 전체 설정은 독립 묶음으로 본문 전체를 쓰며, 그 밖에 어느 묶음에도 속하지 않으면 null이다.
     */
    rootOf(
        widget,
    ) {
        let root = null;
        const folder = widget ? this.workspace.workFolderOf(widget) : null;
        if (widget?.id === PreferencesWidget.ID) {
            root = PreferencesWidget.ID;
        } else if (folder) {
            root = `folder:${folder}`;
        } else if (widget instanceof WebviewWidget && this.workspace.webviewFolders.has(widget.id)) {
            root = `folder:${this.workspace.webviewFolders.get(widget.id)}`;
        } else {
            const top = widget ? this.workspace.topTerminalOf(widget) : null;
            if (top) root = `top:${top}`;
        }
        return root;
    }

    /**
     * 본문에 `root` 묶음만 보여야 하는데 그렇지 않은지. 다른 묶음이 화면에 있거나, 위젯이 화면에서 빠져 있으면 참이다.
     */
    needsRootSwitch(
        root,
        widget,
    ) {
        const shown = layoutModel.widgetsOf(this.workspace.shell.mainPanel.saveLayout().main);
        return !shown.includes(widget) || this.workspace.displayedRoot !== root || shown.some(item => (this.rootOf(item) ?? root) !== root);
    }

    /**
     * 본문에 `root` 묶음의 칸 배치만 보이게 한다.
     * 지금 배치는 묶음별로 나눠 기억하고, 다른 묶음의 위젯은 닫지 않고 화면에서만 뺀다(터미널 프로세스는 계속 산다).
     * 돌아오면 그 묶음이 떠날 때의 칸 나눔과 칸별 선택 탭 그대로 나타난다. 처음 보는 묶음은 위젯 하나짜리 칸이다.
     */
    showRoot(
        root,
        widget,
    ) {
        const panel = this.workspace.shell.mainPanel;
        if (root === PreferencesWidget.ID && this.workspace.displayedRoot !== root) {
            this.workspace.preferencesReturnRoot = this.workspace.displayedRoot;
        }
        this.workspace.isSwitchingRoot = true;
        try {
            const full = panel.saveLayout().main;
            // 독립 묶음이 없는 보조 화면은 지금 보이던 묶음과 함께 움직인다.
            const fallback = this.workspace.displayedRoot ?? root;
            const groupOf = item => this.rootOf(item) ?? fallback;
            for (const name of new Set(layoutModel.widgetsOf(full).map(groupOf))) {
                let area = layoutModel.prune(this.workspace.rootLayouts.get(name), item => !item.isDisposed && !item.secondaryWindow && (this.rootOf(item) ?? name) === name);
                if ((!this.workspace.isRestoringRootLayouts && name === this.workspace.displayedRoot) || !area) {
                    area = layoutModel.prune(full, item => groupOf(item) === name);
                    // 설정 탭을 끼워 넣으며 바뀐 선택 대신 설정을 열기 직전 파일·터미널을 기억한다.
                    if (root === PreferencesWidget.ID && name === this.workspace.preferencesReturnRoot && this.workspace.preferencesReturnWidget) {
                        area = layoutModel.select(area, this.workspace.preferencesReturnWidget);
                    }
                } else {
                    // 다른 묶음에 터미널을 추가하면 새 위젯만 본문에 먼저 들어온다. 그것으로 보관 중인 전체 배치를 덮지 않는다.
                    for (const item of layoutModel.widgetsOf(full).filter(item => groupOf(item) === name)) {
                        if (!layoutModel.includes(area, item)) area = layoutModel.withWidget(area, item);
                    }
                }
                this.workspace.rootLayouts.set(name, area);
            }
            let next = layoutModel.prune(this.workspace.rootLayouts.get(root), item => !item.isDisposed && !item.secondaryWindow && (this.rootOf(item) ?? root) === root);
            if (widget && !layoutModel.includes(next, widget)) next = layoutModel.withWidget(next, widget);
            if (widget) next = layoutModel.select(next, widget);
            if (next) panel.restoreLayout({ main: next });
            this.workspace.displayedRoot = root;
            this.workspace.isRestoringRootLayouts = false;
        } finally {
            this.workspace.isSwitchingRoot = false;
        }
    }

    /** 위젯의 묶음이 본문에 온전히 보이도록 맞춘다. 이미 맞으면 아무것도 하지 않는다. */
    showRootOf(
        widget,
    ) {
        // 저장된 묶음 소속을 읽기 전에는 모든 터미널이 제각각 상단 묶음으로 보여, 되살린 분할 배치를 터미널별로 쪼갠다.
        const root = widget && !widget.secondaryWindow && this.workspace.isGroupsRestored && !this.workspace.isSwitchingRoot ? this.rootOf(widget) : null;
        if (root && this.needsRootSwitch(root, widget)) this.showRoot(root, widget);
    }

    /** 저장된 묶음별 비율을 되살린 위젯에 연결한다. 복원되지 않는 화면만 빼고 나머지 칸 비율은 유지한다. */
    async restoreRootLayouts(
        shownWidgetId,
    ) {
        const saved = await this.workspace.storage.getData(STORAGE.ROOT_LAYOUTS, []).catch(() => []);
        const widgets = this.workspace.shell.widgets.filter(widget => !widget.isDisposed);
        const used = new Set();
        let filesChanged = false;
        let shownWidget;
        const resolveWidget = reference => {
            let widget = widgets.find(item => item.id === reference.id);
            if (!widget && reference.uri) {
                // 이미지 등 확장 편집기는 재시작 때 id가 바뀐다. 같은 파일·같은 편집기인 화면 하나만 연결한다.
                const matches = widgets.filter(item => item.getResourceUri?.()?.toString() === reference.uri
                    && (item.viewType || '') === (reference.viewType || ''));
                if (matches.length === 1) widget = matches[0];
            }
            if (widget && used.has(widget)) widget = undefined;
            if (widget) {
                used.add(widget);
                if (reference.id === shownWidgetId) shownWidget = widget;
                if (widget.id !== reference.id && this.workspace.fileRoots.has(reference.id)) {
                    this.workspace.fileRoots.set(widget.id, this.workspace.fileRoots.get(reference.id));
                    this.workspace.fileRoots.delete(reference.id);
                    filesChanged = true;
                }
            }
            return widget;
        };
        if (Array.isArray(saved)) {
            for (const entry of saved) {
                if (Array.isArray(entry) && typeof entry[0] === 'string') {
                    const area = layoutModel.restore(entry[1], resolveWidget);
                    if (area) {
                        const root = entry[0];
                        this.workspace.rootLayouts.set(root, area);
                        for (const widget of layoutModel.widgetsOf(area)) {
                            if (root.startsWith('top:') && !this.workspace.isTerminal(widget) && this.workspace.fileRoots.get(widget.id) !== root.slice(4)) {
                                this.workspace.fileRoots.set(widget.id, root.slice(4));
                                filesChanged = true;
                            } else if (root.startsWith('folder:') && widget instanceof WebviewWidget) {
                                this.workspace.webviewFolders.set(widget.id, root.slice(7));
                            }
                        }
                    }
                }
            }
        }
        this.workspace.isRestoringRootLayouts = this.workspace.rootLayouts.size > 0;
        if (filesChanged) await this.workspace.saveInnerTabs();
        return shownWidget;
    }
}

module.exports = { RootSwitcher };
