// 설치한 확장의 README를 확장 페이지에 보이게 한다. 화면 모듈(frontend-module.js)과 분리해 Theia 없이 시험한다.

/**
 * 확장 모델의 요청 중 이 앱 백엔드가 내주는 주소(설치한 확장의 README)만 화면에서 직접 받도록 감싼다.
 *
 * 설치한 확장의 페이지는 README를 `http://localhost:<백엔드 포트>/hostedPlugin/<확장>/readme.md`에서 받는다.
 * Theia 확장 모델은 이 주소를 같은 출처라 보고 요청 서비스로 받는데, Electron에서는 요청 서비스가 백엔드를 거치고
 * 백엔드는 허용 목록(확장 레지스트리 주소)에 없는 주소를 거부한다. 그래서 설치한 확장의 페이지는 머리만 보이고 본문이 비었다.
 * 앱 백엔드 주소(`localOrigin`)로 시작하는 요청만 화면의 `fetch`로 받고, 나머지(레지스트리 등)는 원래 요청 서비스로 보낸다.
 * 백엔드 허용 목록은 넓히지 않는다 — 화면은 원래 이 주소를 직접 읽을 수 있으므로 새로 열리는 경로가 없다.
 *
 * 모델은 하나라 만들어지는 순간(DI 활성화) 한 번 감싼다. 같은 모델을 다시 넘겨도 두 번 감싸지 않는다. 감싼 모델을 그대로 돌려준다.
 * 직접 받은 응답은 요청 서비스와 같은 모양(`res.statusCode`·`res.headers`·`buffer`)으로 돌려줘, 실패 상태 코드도 모델이 원래대로 처리한다.
 *
 * Examples:
 *   요청 주소                                            → 받는 쪽
 *   http://localhost:38053/hostedPlugin/a_b/readme.md   → fetch (localOrigin이 http://localhost:38053일 때)
 *   https://open-vsx.org/api/a/b/1.0.0/file/readme.md   → 원래 요청 서비스
 */
function withLocalReadme(
    model,
    localOrigin,
    fetchUrl = globalThis.fetch,
) {
    if (model && model.request && !model.paddockLocalReadme) {
        const original = model.request;
        model.paddockLocalReadme = true;
        // 요청 서비스는 Electron에서 백엔드 호출 대리 객체라 상속하지 않고, 쓰는 메서드를 하나씩 넘긴다.
        model.request = {
            configure: config => original.configure(config),
            resolveProxy: url => original.resolveProxy(url),
            request: async (options, token) => {
                let context;
                if (isLocalUrl(options?.url, localOrigin)) {
                    const response = await fetchUrl(options.url);
                    context = {
                        url: options.url,
                        res: { statusCode: response.status, headers: Object.fromEntries(response.headers) },
                        buffer: new Uint8Array(await response.arrayBuffer()),
                    };
                } else {
                    context = await original.request(options, token);
                }
                return context;
            },
        };
    }
    return model;
}

/** 주소가 `localOrigin`(프로토콜·호스트·포트)과 같은 출처면 참이다. 주소가 아니거나 출처가 다르면 거짓이다. */
function isLocalUrl(
    url,
    localOrigin,
) {
    let local = false;
    try {
        local = typeof url === 'string' && new URL(url).origin === new URL(localOrigin).origin;
    } catch {
        local = false;
    }
    return local;
}

module.exports = { withLocalReadme, isLocalUrl };
