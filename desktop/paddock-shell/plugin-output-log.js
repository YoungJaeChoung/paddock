// 확장의 출력 채널(Output 보기) 중 오류·경고 줄을 로그(backend.log)로도 보낸다.
// 내장 git 확장의 'Git' 채널은 실행한 명령과 stderr를 앱 안에서만 보여 주고 파일로 남기지 않아,
// 앱이 닫힌 뒤에는 어떤 git 명령이 왜 실패했는지 알 수 없었다.

/** 로그 수준 접두가 붙은 출력 줄에서 로그로 남길 수준. Theia의 로그 채널은 `<시각> [Error] <내용>` 꼴로 쓴다. */
const LOGGED_LEVEL_PATTERN = /^\S+ \[(error|warning)\] /i;

/** 로그로 보낼 채널 이름. 다른 확장 채널은 양이 많고 오류 표지 규칙이 제각각이라 'Git'만 다룬다. */
const LOGGED_CHANNEL = 'Git';

/**
 * 출력 채널에 덧붙는 글에서 로그로 남길 줄(오류·경고)만 골라 돌려준다.
 *
 * 한 번의 덧붙임에 여러 줄이 올 수 있어 줄 단위로 거른다. 대상 채널이 아니거나 해당 줄이 없으면 빈 배열이다.
 *
 * Examples
 * --------
 * | name   | text                                         | 결과                        |
 * |--------|----------------------------------------------|-----------------------------|
 * | 'Git'  | '2026-10-14T00:00:00Z [Error] fatal: x\n'    | ['… [Error] fatal: x']      |
 * | 'Git'  | '2026-10-14T00:00:00Z [Info] > git status\n' | []                          |
 * | 'Tasks'| '2026-10-14T00:00:00Z [Error] x\n'           | []                          |
 */
function loggedLines(
    name,
    text,
) {
    let lines = [];
    if (name === LOGGED_CHANNEL && typeof text === 'string') {
        lines = text.split('\n').filter(line => LOGGED_LEVEL_PATTERN.test(line));
    }
    return lines;
}

/**
 * 출력 채널 등록기의 `$append`를 감싸 오류·경고 줄을 `logger.warn`으로도 보낸다. 채널 보기에 쓰는 동작은 그대로다.
 * 같은 줄이 쏟아져도 상한은 백엔드의 반복 억제(log-squelch.js)가 지킨다.
 */
function withOutputLog(
    registry,
    logger,
) {
    const append = registry.$append.bind(registry);
    registry.$append = (name, text, pluginInfo) => {
        for (const line of loggedLines(name, text)) logger.warn(`[git] ${line}`);
        return append(name, text, pluginInfo);
    };
    return registry;
}

module.exports = { loggedLines, withOutputLog };
