// Paddock 창의 고정 탐색 요소. 편집기와 별도로 유지한다.
function element(
    html,
) {
    const template = document.createElement("template");
    template.innerHTML = html.trim();
    return template.content.firstElementChild;
}
module.exports = {
    // 위쪽 줄은 작업 폴더에 속하지 않는 추가 터미널만 보여 준다. 작업 터미널은 사이드바 Work 보기에 있다.
    header: () => element(`<header class="paddock-tabbar" aria-label="Terminals outside work folders">
        <div class="window-controls-space" aria-hidden="true"></div>
        <button type="button" class="sidebar-toggle" aria-label="Hide sidebar" aria-controls="paddock-sidebar" aria-expanded="true" title="Hide sidebar"><span class="codicon codicon-layout-sidebar-left" aria-hidden="true"></span></button>
        <button type="button" class="tabs-scroll" data-direction="-1" aria-label="Show earlier terminal groups" title="Show earlier terminal groups" hidden><span class="codicon codicon-chevron-left"></span></button>
        <div class="tab-strip" id="tab-strip" role="tablist" aria-label="Terminals outside work folders"></div>
        <button type="button" class="tabs-scroll" data-direction="1" aria-label="Show later terminal groups" title="Show later terminal groups" hidden><span class="codicon codicon-chevron-right"></span></button>
        <div class="tab-actions">
            <button type="button" class="tab-add" aria-label="New terminal at ~ (not in a work folder)" title="New terminal at ~ (not in a work folder)"><span class="codicon codicon-add"></span></button>
            <button type="button" class="shell-picker" aria-label="Choose shell" title="Choose shell" popovertarget="shell-menu" aria-haspopup="menu"><span class="codicon codicon-chevron-down"></span></button>
        </div>
        <div class="window-drag-space" aria-hidden="true"></div>
        <div class="window-controls" aria-label="Window controls">
            <button type="button" class="window-minimize" aria-label="Minimize window" title="Minimize"><svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2 8.5h8" /></svg></button>
            <button type="button" class="window-maximize" aria-label="Maximize window" title="Maximize"><svg class="icon-maximize" viewBox="0 0 12 12" aria-hidden="true"><rect x="2.25" y="2.25" width="7.5" height="7.5" rx=".5" /></svg><svg class="icon-restore" viewBox="0 0 12 12" aria-hidden="true"><path d="M4 3V2h6v6H9M2 4h6v6H2z" /></svg></button>
            <button type="button" class="window-close" aria-label="Close window" title="Close"><svg viewBox="0 0 12 12" aria-hidden="true"><path d="m2.5 2.5 7 7m0-7-7 7" /></svg></button>
        </div>
        <div id="shell-menu" popover role="menu" aria-label="Shell for a new terminal"></div>
    </header>`),
    // 본문 바로 위 줄은 선택한 묶음의 내부 터미널과 파일을 보여 준다. ＋는 같은 묶음에 터미널을 연다.
    // 폴더 이름은 두지 않는다 — 사이드바에서 그 폴더 줄이 강조돼 있어 같은 이름이 두 번 보인다(＋ 말풍선에만 적는다).
    folderBar: () => element(`<div class="folder-bar" aria-label="Internal terminals and files">
        <button type="button" class="tabs-scroll" data-direction="-1" aria-label="Show earlier tabs" title="Show earlier tabs" hidden><span class="codicon codicon-chevron-left"></span></button>
        <div class="folder-tabs" role="tablist" aria-label="Internal terminals and files"></div>
        <button type="button" class="tabs-scroll" data-direction="1" aria-label="Show later tabs" title="Show later tabs" hidden><span class="codicon codicon-chevron-right"></span></button>
        <div class="tab-actions">
            <button type="button" class="folder-tab-add" aria-label="New terminal here" title="New terminal here"><span class="codicon codicon-add"></span></button>
        </div>
        <div class="folder-bar-space"></div>
        <div class="markdown-view-actions" role="group" aria-label="Markdown view" hidden>
            <button type="button" data-markdown-view="file" aria-pressed="true">File</button>
            <button type="button" data-markdown-view="preview" aria-pressed="false">Preview</button>
            <button type="button" data-markdown-view="both" aria-pressed="false">Both</button>
        </div>
        <button type="button" class="folder-bar-branch" title="Switch branch" hidden><span class="codicon codicon-git-branch"></span><span class="folder-bar-branch-name"></span></button>
        <div class="tab-actions folder-bar-actions">
            <button type="button" class="folder-split-down" aria-label="New terminal below" title="New terminal below (Ctrl+Shift+\`)"><span class="codicon codicon-split-vertical"></span></button>
            <button type="button" class="folder-split-right" aria-label="New terminal to the side" title="New terminal to the side (Ctrl+Shift+C)"><span class="codicon codicon-split-horizontal"></span></button>
        </div>
    </div>`),
    sidebar: () => element(`<aside class="sidebar" id="paddock-sidebar" aria-label="Sidebar">
        <nav class="view-bar" aria-label="Sidebar views">
            <button type="button" class="view-button is-active" data-view="work" aria-label="Work" title="Work"><span class="codicon codicon-list-tree"></span></button>
            <button type="button" class="view-button" data-view="scm" aria-label="Source control" title="Source control"><span class="codicon codicon-source-control"></span><span class="view-badge" hidden></span></button>
            <button type="button" class="view-button" data-view="extensions" aria-label="Extensions" title="Extensions"><span class="codicon codicon-extensions"></span></button>
            <span class="view-bar-space"></span>
            <button type="button" class="view-button quick-settings-button" aria-label="Quick settings" title="Quick settings — appearance, shell and status bar" popovertarget="quick-settings" aria-haspopup="dialog"><span class="codicon codicon-settings-gear"></span></button>
        </nav>
        <div id="quick-settings" popover role="dialog" aria-label="Quick settings"></div>
        <div class="view-host" data-host="work" id="sidebar-content"></div>
        <div class="view-host" data-host="scm" hidden></div>
        <div class="view-host" data-host="extensions" hidden></div>
    </aside>`),
    footer: () => element(`<footer class="statusbar">
        <div class="status-left">
            <button type="button" class="remote-indicator" aria-label="Open a remote window" title="Open a remote window"><span class="codicon codicon-remote"></span><span class="remote-label"></span></button>
            <span id="statusbar-right" role="status">Ready</span>
        </div>
        <div class="status-right">
            <span class="ai-usage"></span>
            <span class="memory-usage"></span>
        </div>
    </footer>`),
};
