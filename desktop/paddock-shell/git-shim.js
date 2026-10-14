// Git 보기(내장 git 확장)의 `git.path` 기본값을 WSL 폴더용 git 분배기로 정한다.
const { PreferenceSchemaService } = require('@theia/core/lib/common/preferences/preference-schema');
const { Endpoint } = require('@theia/core/lib/browser/endpoint');

/**
 * 백엔드가 알려 준 분배기 경로를 `git.path`의 기본값으로 등록한다. 사용자가 설정에서 `git.path`를 직접 정하면 그 값이 우선한다.
 *
 * 왜 화면에서 정하는가: `git.path`는 확장이 화면의 설정 서비스에서 읽는다. 확장이 처음 읽기 전에 등록해야 하므로 `configure`에서 한다
 * (확장 로딩은 모든 `configure`가 끝난 뒤 시작한다). 백엔드가 응답하지 않거나 분배기가 없으면 아무것도 바꾸지 않아 Windows git을 쓴다.
 */
class PaddockGitShim {
    constructor(
        container,
    ) {
        this.container = container;
    }

    async configure() {
        try {
            const response = await fetch(new Endpoint({ path: '/paddock/session/git-shim' }).getRestUrl().toString(), { signal: AbortSignal.timeout(3000) });
            const { path: shimPath } = response.ok ? await response.json() : {};
            if (typeof shimPath === 'string' && shimPath) {
                this.container.get(PreferenceSchemaService).registerOverride('git.path', undefined, shimPath);
            }
        } catch (error) {
            console.warn('Could not look up the Git shim path; Git view uses the Windows git.', error);
        }
    }
}

module.exports = { PaddockGitShim };
