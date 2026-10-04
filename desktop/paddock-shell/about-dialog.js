const React = require('@theia/core/shared/react');
const { decorate, injectable } = require('@theia/core/shared/inversify');
const { AboutDialog } = require('@theia/core/lib/browser/about-dialog');

/** 앱 정보의 버전·확장 목록을 유지하며 터미널과 외부 통신의 범위를 함께 설명한다. */
class PaddockAboutDialog extends AboutDialog {
    renderHeader() {
        return React.createElement(React.Fragment, null,
            super.renderHeader(),
            React.createElement('h3', null, 'Privacy'),
            React.createElement('p', null, 'Paddock does not collect terminal commands or file contents. AI usage is read from local files.'),
            React.createElement('p', null, 'Extension searches are sent to Open VSX. Commands you run and extensions you install may contact external services and access local files.'),
        );
    }
}

decorate(injectable(), PaddockAboutDialog);
module.exports = { PaddockAboutDialog };
