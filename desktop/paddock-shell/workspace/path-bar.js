const { WebviewWidget } = require('@theia/plugin-ext/lib/main/browser/webview/webview');
const model = require('../work-model');
const { h } = require('@theia/core/shared/@lumino/virtualdom');
const { paneColumns } = require('../pane-columns');
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
     * 칸을 나눠도 위쪽 공통 탭 줄은 그대로 둔다. 탭이 칸 위로 내려가면 위쪽 줄이 빈 줄처럼 보이기 때문이다. 대신 위쪽 줄의 탭은
     * 그 화면이 든 칸의 가로 구간 위에 놓인다(folder-tabs.js의 arrangeTabs).
     * 분할 중 위쪽 줄에 없는 화면(작업 폴더 밖 파일·확장 페이지 등)이 든 칸만 실제 탭을 보여 칸 사이 드래그 이동과 합치기를 지원한다.
     * 이때 실제 탭 줄에는 위쪽 줄에 없는 화면의 탭만 남긴다(같은 터미널 탭이 두 줄에 나오지 않게).
     * 위아래로 쌓인 열의 둘째 칸부터는 탭이 위쪽 줄에 없으므로 그 칸의 실제 탭 줄을 칸 바로 위에 보인다.
     * 위쪽 줄에 이미 있는 화면만 든 열의 맨 위 칸은 탭이 바로 위에 있어 이름 줄을 접는다(같은 이름 두 번 금지).
     * 한 칸에서도 이름 줄을 접고 공통 탭 줄만 쓰며, 설정 화면 등에는 경로 줄을 둔다.
     */
    renderPathBars() {
        let isResized = false;
        const isSplit = [...this.workspace.shell.mainPanel.tabBars()].length > 1;
        // 위쪽 줄은 이 함수보다 먼저 그려진다(작업 화면 갱신 순서). 그 탭이 가리키는 화면을 모은다.
        const listedIds = new Set([...this.workspace.shell.folderBar.node.querySelectorAll('.folder-tabs [data-widget-id]')].map(tab => tab.dataset.widgetId));
        // 열의 맨 위 칸은 위쪽 줄의 탭이 바로 위에 있다. 둘째 칸부터는 위쪽 줄에 탭이 없어 자기 탭 줄을 둔다.
        const columns = paneColumns(this.workspace.folderTabs.paneRects());
        const topPanes = new Set(columns.map(column => column.keys[0]));
        // 위아래로 쌓인 열의 둘째 칸부터는 탭이 위쪽 줄에 없다(folder-tabs.js의 arrangeTabs). 그 칸의 실제 탭 줄을 칸 위에 보인다.
        const lowerPanes = new Set(columns.flatMap(column => column.keys.slice(1)));
        for (const tabBar of this.workspace.shell.mainPanel.tabBars()) {
            const showsTabs = isSplit && (lowerPanes.has(tabBar) || tabBar.titles.some(title => !listedIds.has(title.owner.id)));
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
                const names = title.className.split(' ').filter(name => name && name !== 'has-unseen-activity' && name !== 'is-listed');
                if (this.workspace.doneIds.has(title.owner.id)) names.push('has-unseen-activity');
                // 위쪽 줄에 이미 있는 화면의 탭은 실제 탭 줄에서 숨겨(style.css) 같은 이름 탭이 두 줄에 나오지 않게 한다.
                if (listedIds.has(title.owner.id)) names.push('is-listed');
                title.className = names.join(' ');
            }
            const widget = tabBar.currentTitle?.owner;
            let bar = tabBar.node.querySelector(':scope > .path-bar');
            if (!bar) {
                bar = element('div', 'path-bar');
                tabBar.node.append(bar);
            }
            bar.replaceChildren();
            // 한 칸이거나 열의 맨 위 칸은 위쪽 탭 줄의 탭이 이름을 맡으므로 줄을 1px로 접는다. 아래 칸은 자기 실제 탭 줄이 이름을 맡는다.
            const hasTabAbove = !isSplit || (topPanes.has(tabBar) && !showsTabs);
            const isCompactPane = hasTabAbove && (this.workspace.isTerminal(widget) || Boolean(widget?.getResourceUri?.()) || widget instanceof WebviewWidget && (this.workspace.webviewFolders.has(widget.id) || this.workspace.fileRoots.has(widget.id)));
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
                // 브랜치는 위쪽 줄의 브랜치 버튼이 보이므로 위쪽 줄에 없는 화면에만 적는다(같은 정보 두 번 금지).
                if (parts.branch && !listedIds.has(widget.id)) bar.append(codicon('git-branch'), element('span', 'path-branch', parts.branch));
                // 닫기는 위쪽 탭 줄의 ×가 맡는다. 위쪽 줄에 없는 화면(설정 등)만 이름 줄에 닫기를 둔다.
                if (!listedIds.has(widget.id)) {
                    const actions = element('span', 'path-actions');
                    const close = button([codicon('close')], 'path-action', () => this.workspace.run(() => this.workspace.closeWidget(widget)));
                    close.title = 'Close';
                    actions.append(close);
                    bar.append(actions);
                }
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
        // 터미널은 셸 이름(bash)이 아니라 위쪽 탭·Work 목록과 같은 이름(claude·terminal 2)을 쓴다.
        const item = uri ? (key ? uri.toString().slice(key.length + 1) : uri.path.base) : this.workspace.isTerminal(widget) ? this.workspace.terminalDisplayName(widget) : widget.title.label;
        const folder = uri ? (key ? this.workspace.folderName(key) : 'File') : '';
        return { folder, item: decodeURIComponent(item), branch: key ? this.workspace.branchOf(key) : '' };
    }
}

module.exports = { PathBar };
