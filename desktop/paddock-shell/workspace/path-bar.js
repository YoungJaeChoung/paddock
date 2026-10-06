const { WebviewWidget } = require('@theia/plugin-ext/lib/main/browser/webview/webview');
const model = require('../work-model');
const { h } = require('@theia/core/shared/@lumino/virtualdom');
const { element, codicon, button } = require('./shared');

/**
 * 본문이 한 칸일 때 탭 대신 보이는 경로 줄(폴더·브랜치·파일 이름)을 그린다.
 */
class PathBar {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
        this.namedTabRenderers = new WeakSet();
    }

    /**
     * 본문 칸마다 생기는 탭 줄을 꾸민다.
     *
     * 칸을 나눠도 위쪽 공통 탭 줄은 그대로 둔다. 파일을 옆 칸에 열 때마다 탭이 칸 위로 내려가면 탭 위치가 바뀌어 보이기 때문이다.
     * 분할 중 위쪽 줄에 없는 화면(작업 폴더 밖 파일 등)이 든 칸만 실제 탭을 보여 칸 사이 드래그 이동과 합치기를 지원하고,
     * 위쪽 줄에 이미 있는 화면만 든 칸은 줄을 접는다. 한 칸에서는 공통 탭 줄을 쓰고, 설정 화면 등에는 경로 줄을 둔다.
     */
    renderPathBars() {
        let isResized = false;
        const isSplit = [...this.workspace.shell.mainPanel.tabBars()].length > 1;
        // 위쪽 줄은 이 함수보다 먼저 그려진다(작업 화면 갱신 순서). 그 탭이 가리키는 화면을 모은다.
        const listedIds = new Set([...this.workspace.shell.folderBar.node.querySelectorAll('.folder-tabs [data-widget-id]')].map(tab => tab.dataset.widgetId));
        for (const tabBar of this.workspace.shell.mainPanel.tabBars()) {
            const showsTabs = isSplit && tabBar.titles.some(title => !listedIds.has(title.owner.id));
            tabBar.node.classList.toggle('shows-tabs', showsTabs);
            if (!this.namedTabRenderers.has(tabBar.renderer)) {
                this.namedTabRenderers.add(tabBar.renderer);
                const renderLabel = tabBar.renderer.renderLabel.bind(tabBar.renderer);
                tabBar.renderer.renderLabel = (data, side) => {
                    const terminal = data.title.owner;
                    return this.workspace.isTerminal(terminal) && !side
                        ? h.div({ className: 'lm-TabBar-tabLabel' }, this.workspace.terminalDisplayName(terminal))
                        : renderLabel(data, side);
                };
            }
            tabBar.update();
            for (const title of tabBar.titles) {
                const names = title.className.split(' ').filter(name => name && name !== 'has-unseen-activity');
                if (this.workspace.doneIds.has(title.owner.id)) names.push('has-unseen-activity');
                title.className = names.join(' ');
            }
            const widget = tabBar.currentTitle?.owner;
            let bar = tabBar.node.querySelector(':scope > .path-bar');
            if (!bar) {
                bar = element('div', 'path-bar');
                tabBar.node.append(bar);
            }
            bar.replaceChildren();
            const isCompactPane = !showsTabs && (this.workspace.isTerminal(widget) || Boolean(widget?.getResourceUri?.()) || widget instanceof WebviewWidget && (this.workspace.webviewFolders.has(widget.id) || this.workspace.fileRoots.has(widget.id)));
            // 칸이 둘 이상이면 입력이 가지 않는 칸을 흐리게 해 지금 입력할 칸을 드러낸다.
            widget?.node.classList.toggle('is-inactive-pane', isSplit && widget !== this.workspace.currentWidget());
            isResized = isResized || tabBar.node.classList.contains('is-compact-pane') !== isCompactPane;
            tabBar.node.classList.toggle('is-compact-pane', isCompactPane);
            if (widget) {
                bar.classList.toggle('is-active', widget === this.workspace.currentWidget() && (!isCompactPane || isSplit));
                bar.onclick = () => this.workspace.run(() => this.workspace.activate(widget.id));
            }
            if (widget && !isCompactPane) {
                const parts = this.filePathParts(widget);
                // 파일이 아닌 화면(설정 등)은 폴더 없이 화면 이름만 보인다.
                if (parts.folder) bar.append(codicon('file'), element('span', 'path-folder', parts.folder), codicon('chevron-right'));
                bar.append(element('span', 'path-item', parts.item));
                bar.append(element('span', 'path-space'));
                if (parts.branch) bar.append(codicon('git-branch'), element('span', 'path-branch', parts.branch));
                const actions = element('span', 'path-actions');
                const close = button([codicon('close')], 'path-action', () => this.workspace.run(() => this.workspace.closeWidget(widget)));
                close.title = 'Close';
                actions.append(close);
                bar.append(actions);
            }
        }
        // 화면 이름 줄(30px)과 얇은 구분선(1px)이 바뀌면 본문 칸 배치를 다시 잰다.
        if (isResized) this.workspace.shell.mainPanel.fit();
    }

    /**
     * 파일 칸 경로 줄의 조각: 파일을 품은 작업 폴더 이름(작업 폴더 밖 파일이면 "File"), 그 폴더 기준 상대 경로, 폴더의 브랜치.
     * 파일이 아닌 화면(설정 등)은 폴더가 빈 값이고 화면 이름만 돌려준다.
     */
    filePathParts(
        widget,
    ) {
        const uri = widget.getResourceUri?.();
        const key = uri && model.folderContaining(this.workspace.state, uri.toString());
        const item = uri ? (key ? uri.toString().slice(key.length + 1) : uri.path.base) : widget.title.label;
        const folder = uri ? (key ? this.workspace.folderName(key) : 'File') : '';
        return { folder, item: decodeURIComponent(item), branch: key ? this.workspace.branchOf(key) : '' };
    }
}

module.exports = { PathBar };
