const React = require('@theia/core/shared/react');
const { nls } = require('@theia/core/lib/common/nls');
const { SearchInWorkspaceCommands } = require('@theia/search-in-workspace/lib/browser/search-in-workspace-frontend-contribution');

// Theia 검색(Find in Files)은 열린 작업 공간의 루트만 훑는다. Paddock은 작업 공간 없이 쓰므로 검색이 꺼져 있다.
// 검색 엔진(ripgrep)은 어떤 폴더든 받으므로, 폴더 목록을 대는 자리만 Paddock의 검색 범위(search-scope.js)로 바꾼다.
// Theia 검색 모듈이 Paddock 모듈보다 먼저 묶여 `rebind`할 대상이 없으므로, 만들어지는 순간 인스턴스를 감싼다.

/**
 * 검색 서비스가 Paddock의 검색 범위 폴더를 훑게 한다.
 *
 * 작업 공간이 없어도 켜지고, 폴더가 바뀐 뒤의 검색은 그때의 범위를 새로 읽는다.
 * 범위가 비어 있으면 검색은 오류로 끝난다(Theia 원래 동작과 같다).
 */
function withFolderSearchService(
    service,
    searchRoots,
) {
    service.isEnabled = () => searchRoots().length > 0;
    service.search = async (what, callbacks, opts) => {
        const roots = searchRoots();
        if (roots.length === 0) throw new Error('Search failed: no folder to search.');
        return service.doSearch(what, roots, callbacks, opts);
    };
    return service;
}

/**
 * 검색 위젯의 안내 줄을 "검색 범위: <폴더 이름>"으로 바꾼다.
 *
 * Theia 원래 안내는 작업 공간 루트가 없으면 "폴더를 열지 않았다"고 말해, Paddock에서는 폴더를 검색하는 중에도 틀린 안내가 보인다.
 * 범위가 있으면 범위 줄과 결과 상한 안내를 보이고, 범위가 없으면 Theia 원래 안내를 그대로 둔다.
 */
function withScopeNotice(
    widget,
    scopeName,
) {
    const renderNotification = widget.renderNotification.bind(widget);
    widget.renderNotification = () => {
        const name = scopeName();
        let notification;
        if (name) {
            const isFull = widget.searchInWorkspaceOptions.maxResults && widget.resultNumber >= widget.searchInWorkspaceOptions.maxResults;
            notification = React.createElement(
                React.Fragment,
                null,
                React.createElement('div', { className: 'paddock-search-scope', title: name }, `Searching in ${name}`),
                React.createElement(
                    'div',
                    { className: `search-notification ${isFull ? 'show' : ''}` },
                    React.createElement('div', null, nls.localize('theia/search-in-workspace/resultSubset', 'This is only a subset of all results. Use a more specific search term to narrow down the result list.')),
                ),
            );
        } else {
            notification = renderNotification();
        }
        return notification;
    };
    return widget;
}

/**
 * 검색 보기를 열고 닫는 길을 Paddock 사이드바 파일 영역으로 돌린다.
 *
 * Ctrl+Shift+F·Edit › Find in Files·Replace in Files는 Theia가 쉘의 왼쪽 패널에 보기를 여는데 Paddock 쉘에는 그 패널이 없다.
 * `openView`를 가로채 어느 길로 부르든 사이드바 검색을 보이고(`showSearch`), 두 명령은 검색 범위가 있을 때만 켠다.
 * 시작 때 Theia가 보기를 미리 만들어 두려고 부르는 `openView({ activate: false })`는 사이드바를 건드리지 않는다.
 *
 * @param showSearch 사이드바 검색을 보이는 함수. `{ toggle: true }`이면 이미 보일 때 닫는다.
 */
function withFolderSearchView(
    contribution,
    searchRoots,
    showSearch,
) {
    const registerCommands = contribution.registerCommands.bind(contribution);
    contribution.openView = async (args = {}) => {
        const widget = await contribution.widget;
        if (args.activate || args.reveal || args.toggle) await showSearch({ toggle: Boolean(args.toggle) });
        return widget;
    };
    contribution.registerCommands = async (commands) => {
        await registerCommands(commands);
        // Theia가 등록한 두 명령은 작업 공간 루트가 있어야 켜지므로, 같은 id로 Paddock의 검색 범위 조건으로 다시 등록한다.
        for (const [command, showsReplace] of [[SearchInWorkspaceCommands.OPEN_SIW_WIDGET, false], [SearchInWorkspaceCommands.REPLACE_IN_FILES, true]]) {
            commands.unregisterCommand(command.id);
            commands.registerCommand(command, {
                isEnabled: () => searchRoots().length > 0,
                execute: async () => {
                    const widget = await contribution.openView({ activate: true });
                    widget.updateSearchTerm(contribution.getSearchTerm(), showsReplace);
                },
            });
        }
    };
    return contribution;
}

module.exports = { withFolderSearchService, withScopeNotice, withFolderSearchView };
