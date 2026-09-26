// Herdr 창의 고정 탐색 요소. 편집기와 별도로 유지한다.
function element(
    html,
) {
    const template = document.createElement("template");
    template.innerHTML = html.trim();
    return template.content.firstElementChild;
}
module.exports = {
    // 위쪽 줄은 작업 폴더에 속하지 않는 추가 터미널만 보여 준다. 작업 터미널은 사이드바 Work 보기에 있다.
    header: () => element(`<header class="herdr-tabbar" aria-label="Extra terminals">
        <div class="window-controls-space" aria-hidden="true"></div>
        <div class="tab-strip-label" aria-hidden="true">Extra terminals</div>
        <div class="tab-strip" id="tab-strip" role="tablist" aria-label="Extra terminals"></div>
        <div class="tab-actions">
            <button type="button" class="tab-add" aria-label="New extra terminal" title="New extra terminal"><span class="codicon codicon-add"></span></button>
            <button type="button" class="shell-picker" aria-label="Choose shell" title="Choose shell" popovertarget="shell-menu" aria-haspopup="menu"><span class="codicon codicon-chevron-down"></span></button>
        </div>
        <div class="window-drag-space" aria-hidden="true"></div>
        <div id="shell-menu" popover role="menu" aria-label="Shell for a new terminal"></div>
    </header>`),
    sidebar: () => element(`<aside class="sidebar" aria-label="Sidebar">
        <nav class="view-bar" aria-label="Sidebar views">
            <button type="button" class="view-button is-active" data-view="work" aria-label="Work" title="Work"><span class="codicon codicon-list-tree"></span></button>
            <button type="button" class="view-button" data-view="scm" aria-label="Source control" title="Source control"><span class="codicon codicon-source-control"></span><span class="view-badge" hidden></span></button>
            <button type="button" class="view-button" data-view="extensions" aria-label="Extensions" title="Extensions"><span class="codicon codicon-extensions"></span></button>
        </nav>
        <div class="view-host" data-host="work" id="sidebar-content"></div>
        <div class="view-host" data-host="scm" hidden></div>
        <div class="view-host" data-host="extensions" hidden></div>
    </aside>`),
    footer: () => element(`<footer class="statusbar">
        <div class="status-left">
            <button type="button" class="remote-indicator" aria-label="Open a remote window" title="Open a remote window"><span class="codicon codicon-remote"></span><span class="remote-label"></span></button>
            <span id="statusbar-project">◆ HERDR</span>
        </div>
        <div class="status-right">
            <span id="statusbar-right" role="status">Ready</span>
            <span class="ai-usage"></span>
            <span class="usage-group" data-source="memory"><span class="source-mark">▦</span><span class="source-name">Memory</span><span class="meter" data-meter="memory" title="System memory in use"><span class="meter-bar"><span class="meter-fill"></span></span><span class="meter-percent">—</span></span></span>
        </div>
    </footer>`),
};
