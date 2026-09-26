// Herdr 창의 고정 탐색 요소. 편집기와 별도로 유지한다.
function element(
    html,
) {
    const template = document.createElement("template");
    template.innerHTML = html.trim();
    return template.content.firstElementChild;
}
module.exports = {
    header: () => element(`<header class="herdr-tabbar" aria-label="작업 탭과 창 이동">
        <div class="window-controls-space" aria-hidden="true"></div>
        <div class="tab-strip" id="tab-strip" role="tablist" aria-label="열린 작업"></div>
        <div class="tab-actions">
            <button type="button" class="tab-add" aria-label="새 터미널" title="새 터미널">＋</button>
            <button type="button" class="shell-picker" aria-label="셸 선택" title="셸 선택" popovertarget="shell-menu" aria-haspopup="menu">⌄</button>
        </div>
        <div class="window-drag-space" aria-hidden="true"></div>
        <div id="shell-menu" popover role="menu" aria-label="새 터미널의 셸"></div>
    </header>`),
    rail: () => element("      <nav class=\"activity-rail\" aria-label=\"주요 작업\">\n        <button type=\"button\" class=\"rail-button is-active\" data-action=\"rail\" data-rail=\"terminal\" aria-label=\"터미널\" title=\"터미널\">\n          <svg viewBox=\"0 0 24 24\" aria-hidden=\"true\"><rect x=\"3.5\" y=\"4.5\" width=\"17\" height=\"15\" rx=\"2.5\"/><path d=\"m7.5 9 3 3-3 3M12.8 15.5h4\"/></svg>\n        </button>\n        <button type=\"button\" class=\"rail-button\" data-action=\"rail\" data-rail=\"explorer\" aria-label=\"파일\" title=\"파일\">\n          <svg viewBox=\"0 0 24 24\" aria-hidden=\"true\"><path d=\"M3.5 6.5h6l2 2h9v9.2a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z\"/><path d=\"M3.5 8.5V5.8a2 2 0 0 1 2-2H9\"/></svg>\n        </button>\n        <button type=\"button\" class=\"rail-button\" data-action=\"rail\" data-rail=\"extensions\" aria-label=\"확장\" title=\"확장\">\n          <svg viewBox=\"0 0 24 24\" aria-hidden=\"true\"><path d=\"M9 3.5H5.5a2 2 0 0 0-2 2V9H9zM20.5 9V5.5a2 2 0 0 0-2-2H15V9zM9 15H3.5v3.5a2 2 0 0 0 2 2H9zM15 15v5.5h3.5a2 2 0 0 0 2-2V15z\"/><path d=\"M9 3.5V9H3.5M15 3.5V9h5.5M9 20.5V15H3.5M15 20.5V15h5.5\"/></svg>\n        </button>\n        <div class=\"rail-spacer\"></div>\n        <button type=\"button\" class=\"rail-button rail-button-muted\" data-action=\"about\" aria-label=\"앱 정보\" title=\"앱 정보\">\n          <svg viewBox=\"0 0 24 24\" aria-hidden=\"true\"><circle cx=\"12\" cy=\"12\" r=\"8.5\"/><path d=\"M12 10.5v5.5M12 7.8h.01\"/></svg>\n        </button>\n      </nav>"),
    sidebar: () => element("<aside class=\"sidebar\" aria-label=\"작업 탐색\"><div id=\"sidebar-content\"></div></aside>"),
    footer: () => element("<footer class=\"statusbar\"><span id=\"statusbar-project\">HERDR</span><span id=\"statusbar-right\" role=\"status\">준비됨</span></footer>"),
};
