const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { TerminalWidget } = require('@theia/terminal/lib/browser/base/terminal-widget');
const { Widget, Panel, SplitPanel } = require('@lumino/widgets');
const { injectable, decorate } = require('@theia/core/shared/inversify');
const { isOSX, isWindows } = require('@theia/core/lib/common/os');
const { Saveable } = require('@theia/core/lib/browser/saveable');
const { ConfirmDialog } = require('@theia/core/lib/browser/dialogs');
const markup = require('./shell-markup');
const { widgetArea } = require('./widget-area');

// 사이드바 폭(px). 세로 아이콘 레일은 두지 않고, 보기 전환은 사이드바 위 가로 줄에서 한다.
// 사용자가 경계선을 끌어 바꾼 폭과 접힘 상태는 이 창의 브라우저 저장소에 남아 다음 실행에 되살린다.
class SIDEBAR {
    static DEFAULT_WIDTH = 272;
    static STORAGE_KEY = 'paddock.sidebar-width';
    static COLLAPSED_KEY = 'paddock.sidebar-collapsed';
}

/** 저장된 사이드바 폭. 없거나 읽을 수 없으면 기본 폭이다. */
function savedSidebarWidth() {
    let width = SIDEBAR.DEFAULT_WIDTH;
    try {
        const saved = Number(window.localStorage.getItem(SIDEBAR.STORAGE_KEY));
        if (saved > 0) width = saved;
    } catch {
        // 저장소를 못 쓰는 환경이면 기본 폭으로 시작한다.
    }
    return width;
}

/**
 * Work navigation lives in the sidebar; the body tabs switch terminals and files inside the selected work.
 *
 * 본문은 여러 칸으로 나눌 수 있게 multiple-document 모드로 둔다. 칸마다 생기는 탭 줄은
 * 분할 중에는 실제 탭을 쓰고 한 칸일 때는 경로 줄로 꾸민다(workspace.js가 내용을 채운다).
 */
class PaddockShell extends ApplicationShell {
    createLayout() {
        this.addClass('paddock-shell');
        // macOS 창 버튼이 위쪽 줄 왼쪽에 겹치므로 그 자리를 비울지 CSS가 판단한다.
        this.toggleClass('is-macos', isOSX);
        this.toggleClass('is-linux', !isOSX && !isWindows);
        // 데스크톱 메뉴가 통합 제목줄을 만들 때만 표시한다. macOS는 시스템 메뉴를 쓴다.
        this.topPanel.hide();
        this.mainPanel.mode = 'multiple-document';
        this.header = new Widget({ node: markup.header() });
        this.sidebar = new Widget({ node: markup.sidebar() });
        this.folderBar = new Widget({ node: markup.folderBar() });
        const sidebarToggle = this.folderBar.node.querySelector('.sidebar-toggle');
        sidebarToggle.setAttribute('aria-keyshortcuts', isOSX ? 'Meta+B' : 'Control+B');
        sidebarToggle.addEventListener('click', () => this.toggleSidebar());
        this.footer = new Widget({ node: markup.footer() });
        // Recalculate the workspace when zoom or a narrow window changes the footer's CSS height.
        const footerResize = new ResizeObserver(() => this.fit());
        footerResize.observe(this.footer.node);
        this.disposed.connect(() => footerResize.disconnect());
        // 작업 폴더가 없어도 빈 탭 줄은 남긴다. workspace.js가 선택한 폴더의 탭과 버튼을 채운다.
        const body = new Panel({ layout: this.createBoxLayout(
            [this.folderBar, this.mainPanel], [0, 1], { direction: 'top-to-bottom', spacing: 0 },
        ) });
        // 사이드바와 본문 사이 경계선을 끌어 사이드바 폭을 바꾼다. 창 크기가 바뀌면 본문만 늘고 준다.
        const workspace = new SplitPanel({ orientation: 'horizontal', spacing: 1 });
        workspace.addClass('paddock-workspace-split');
        workspace.addWidget(this.sidebar);
        workspace.addWidget(body);
        SplitPanel.setStretch(this.sidebar, 0);
        SplitPanel.setStretch(body, 1);
        const width = savedSidebarWidth();
        workspace.setRelativeSizes([width, Math.max(window.innerWidth - width, width)]);
        workspace.handleMoved.connect(() => {
            try {
                window.localStorage.setItem(SIDEBAR.STORAGE_KEY, String(Math.round(this.sidebar.node.getBoundingClientRect().width)));
            } catch {
                // 저장하지 못해도 이번 실행에서는 끈 폭 그대로 쓴다.
            }
        });
        let sidebarVisible = true;
        try {
            sidebarVisible = window.localStorage.getItem(SIDEBAR.COLLAPSED_KEY) !== 'true';
        } catch {
            // 저장소를 못 쓰는 환경이면 사이드바를 펼친 채 시작한다.
        }
        this.setSidebarVisible(sidebarVisible);
        const layout = this.createBoxLayout(
            [this.topPanel, this.header, workspace, this.footer], [0, 0, 1, 0], { direction: 'top-to-bottom', spacing: 0 },
        );
        return layout;
    }

    /** 상단 버튼·명령·단축키에서 같은 사이드바 접기와 펼치기를 실행한다. */
    toggleSidebar() {
        this.setSidebarVisible(this.sidebar.isHidden);
    }

    /** 사이드바 내용과 폭을 유지하며 본문 공간을 넓히고, 재열기 버튼과 다음 실행의 상태를 맞춘다. */
    setSidebarVisible(
        visible,
    ) {
        const toggle = this.folderBar.node.querySelector('.sidebar-toggle');
        if (!visible) {
            // 빠른 설정이 열린 채 본체만 사라지지 않도록 함께 닫는다.
            for (const popover of this.sidebar.node.querySelectorAll('[popover]:popover-open')) popover.hidePopover();
            if (this.sidebar.node.contains(document.activeElement)) toggle.focus({ preventScroll: true });
        }
        this.sidebar.setHidden(!visible);
        const label = visible ? 'Hide sidebar' : 'Show sidebar';
        toggle.setAttribute('aria-expanded', String(visible));
        toggle.setAttribute('aria-label', label);
        toggle.title = `${label} (${isOSX ? '⌘B' : 'Ctrl+B'})`;
        toggle.querySelector('.codicon').className = `codicon codicon-layout-sidebar-left${visible ? '' : '-off'}`;
        try {
            window.localStorage.setItem(SIDEBAR.COLLAPSED_KEY, String(!visible));
        } catch {
            // 저장하지 못해도 이번 실행에서는 바꾼 표시 상태를 유지한다.
        }
    }

    async setLayoutData(
        layoutData,
    ) {
        await super.setLayoutData(layoutData);
        // 저장된 배치를 되살리면 모드도 되살아나므로 나눌 수 있는 모드를 다시 적용한다.
        this.mainPanel.mode = 'multiple-document';
    }

    /**
     * 시작 때 쉘에 없는 왼쪽·오른쪽·아래 패널에 들어간 화면을 정리하고, 이후 그 패널을 요청하는 화면은 본문으로 돌리기 시작한다.
     * 사이드바 보기는 사이드바에 붙이고, 나머지는 닫는다. 패널에 남겨 두면 Theia 명령이 "이미 열려 있다"고 보고 새로 넣지 않아 아무것도 안 보이고,
     * 닫아 두면 명령이 `addWidget`으로 본문에 새로 연다. 배치를 되살리는 동안은 Theia가 패널에 넣는 대로 두고(시작 때 본문에 낯선 탭이 생기지 않게) 끝난 뒤 한 번 부른다.
     */
    async releasePanelWidgets() {
        for (const area of ['left', 'right', 'bottom']) {
            for (const widget of [...this.getWidgets(area)]) {
                const view = this.sidebarViewOf?.(widget) ?? null;
                if (view) await this.showSidebarView?.(view);
                else if (!(widget instanceof TerminalWidget)) widget.close();
            }
        }
        this.redirectsPanels = true;
    }

    /**
     * 위젯을 선택하기 전에 그 위젯의 묶음 배치를 본문에 되살린다(`beforeActivate`는 workspace.js가 건다).
     * Theia 명령이나 다른 확장이 직접 선택해도, 화면에서 빠져 있던 다른 묶음의 터미널이 본문에 나타난다.
     */
    async activateWidget(
        id,
    ) {
        const widget = this.getWidgetById(id);
        if (widget?.secondaryWindow) {
            // The main document's focus tracker cannot wait for focus in a
            // different native window. Select the existing view there directly.
            widget.secondaryWindow.focus();
            widget.activate();
            return widget;
        }
        let result = widget;
        // 사이드바 보기는 쉘의 패널 밖에 있어 Theia의 선택 경로가 "붙어 있지 않다"며 실패하므로 위젯을 직접 선택한다.
        if (await this.placeInBody(widget)) {
            widget.activate();
        } else {
            this.beforeActivate?.(id);
            result = await super.activateWidget(id);
        }
        return result;
    }

    async revealWidget(
        id,
    ) {
        const widget = this.getWidgetById(id);
        return await this.placeInBody(widget) ? widget : super.revealWidget(id);
    }

    /**
     * 이미 왼쪽·오른쪽·아래 패널에 들어가 있는 화면을 선택하거나 드러낼 때 본문 탭으로 옮긴다(사이드바 보기는 사이드바로 보낸다).
     * 시작 때 패널에 미리 넣어 둔 보기는 `addWidget`을 다시 거치지 않으므로, 명령으로 열어도 보이는 자리가 없는 채로 남는다.
     * 사이드바 보기를 사이드바로 보냈으면 true, 아니면 false를 돌려준다(호출자는 true일 때 Theia의 선택 경로를 건너뛴다).
     */
    async placeInBody(
        widget,
    ) {
        let isSidebarView = false;
        if (widget && !widget.secondaryWindow) {
            const view = this.sidebarViewOf?.(widget) ?? null;
            const area = this.getAreaFor(widget);
            const target = widgetArea({
                area,
                isTerminal: false,
                isSidebarView: Boolean(view),
                redirectsPanels: true,
            });
            isSidebarView = Boolean(view);
            if (view) await this.showSidebarView?.(view);
            else if (target !== area) await this.addWidget(widget, { area: target });
        }
        return isSidebarView;
    }

    async addWidget(
        widget,
        options,
    ) {
        // 터미널은 아래 패널이 아니라 본문 칸에 연다. 본문이 이미 터미널이라 자리를 하나로 둔다.
        // 검색·출력·문제 같은 화면도 쉘에 없는 왼쪽·오른쪽·아래 패널 대신 본문 탭으로 연다. 사이드바가 품는 보기(`sidebarViewOf`는 workspace.js가 건다)만 사이드바로 보낸다.
        const view = this.sidebarViewOf?.(widget) ?? null;
        const area = widgetArea({
            area: options?.area,
            isTerminal: widget instanceof TerminalWidget,
            isSidebarView: Boolean(view),
            redirectsPanels: Boolean(this.redirectsPanels),
        });
        const target = area === options?.area ? options : { ...options, area };
        // 이미 사이드바에 붙은 보기를 패널에 다시 넣으면 붙임 상태만 어긋나 사이드바로 되돌릴 수 없으므로, 넣지 않고 사이드바로만 보낸다.
        if (!(view && this.sidebar.node.contains(widget.node))) await super.addWidget(widget, target);
        if (view) await this.showSidebarView?.(view);
    }

    /** 실행 중인 셸과 명령을 멈추기 전에 대상 터미널 수를 보여 주고 확인한다. */
    async confirmCloseTerminals(
        terminals,
    ) {
        let confirmed = true;
        if (terminals.length) {
            const dialog = new ConfirmDialog({
                // 하나면 어느 터미널인지 이름으로 밝힌다. 여럿이면 수를 보인다.
                title: terminals.length > 1 ? `Close ${terminals.length} terminals` : `Close ${this.terminalName?.(terminals[0]) || terminals[0].title.label}`,
                msg: terminals.length > 1 ? 'The shells and any commands running in these terminals will stop.'
                    : 'The shell and any command running in this terminal will stop.',
                ok: 'Close',
                cancel: 'Cancel',
            });
            // 실행 중인 명령을 멈추는 확인이라 폴더 메뉴의 "Remove from list"와 같은 빨간 색 언어를 쓴다.
            dialog.node.classList.add('is-destructive');
            confirmed = await dialog.open();
        }
        return Boolean(confirmed);
    }

    /** 터미널 종료와 파일 저장을 확인한 뒤 일괄 닫는다. Close All Editors도 같은 종료 확인을 거친다. */
    async closeMany(
        targets,
        options,
    ) {
        const terminals = targets.filter(widget => widget instanceof TerminalWidget);
        let closed = [];
        // 파일의 Don't Save는 즉시 변경을 되돌린다. 터미널 확인에서 취소했을 때 편집 내용도 남도록 먼저 묻는다.
        if (!terminals.length) {
            closed = await super.closeMany(targets, options);
        } else if (await this.confirmCloseTerminals(terminals)
            && (options?.save === false || await Saveable.confirmSaveBeforeClose(targets, this.widgets.filter(widget => !targets.includes(widget))))) {
            closed = (await Promise.all(targets.map(async widget => {
                let result;
                if (widget instanceof TerminalWidget) {
                    await widget.closeWithoutSaving();
                    result = widget;
                } else {
                    result = await super.closeWidget(widget.id, options);
                }
                return result;
            }))).filter(Boolean);
        }
        return closed;
    }
}
decorate(injectable(), PaddockShell);
module.exports = { PaddockShell };
