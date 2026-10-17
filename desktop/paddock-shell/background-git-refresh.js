// Paddock이 사용자 조작 없이 Git 확장에 보내는 저장소 다시 읽기(`git.refresh`)의 실패 알림을 걸러 내는 순수 로직.
// Theia에 의존하지 않아 단위 테스트로 확인한다.

/**
 * 백그라운드 다시 읽기가 진행 중일 때 들어온 Git 오류를 실패 묶음으로 세어, 같은 실패의 알림은 한 번만 띄우게 한다.
 *
 * 왜 있는가: WSL 폴더는 파일 감시가 걸리지 않아 Paddock이 5초마다 `git.refresh` 명령을 보낸다. 명령으로 실행된 다시 읽기가
 * 실패하면 Git 확장은 오류 창을 띄우므로, 같은 실패가 이어지면 5초마다 같은 알림이 다시 뜬다. 확장이 스스로 하는
 * 다시 읽기는 실패해도 조용하다. 같은 실패의 첫 알림은 남겨 Git 보기가 멈췄다는 사실을 알리고, 다시 읽기가 한 번
 * 성공하면 묶음을 닫아 다음 실패를 새로 알린다.
 *
 * 판정 근거: Git 확장은 명령이 실패하면 오류 창 요청을 먼저 보내고 명령의 결과를 그 뒤에 돌려준다. 그래서 백그라운드
 * 다시 읽기의 실패 알림은 그 명령이 끝나기 전에 도착한다. 그 사이에 사용자가 실행한 다른 Git 명령의 오류도 같은 문구면
 * 반복으로 본다(같은 실패이므로 첫 알림으로 충분하다). 문구가 다르면 새 실패로 보고 띄운다.
 *
 * Examples
 * --------
 * | 순서                                                       | classify 결과 |
 * |------------------------------------------------------------|---------------|
 * | 진행 중인 다시 읽기 없음, 'Git: x'                         | 'foreground'  |
 * | 다시 읽기 중 'Git: x'                                      | 'first'       |
 * | 위 뒤 다음 다시 읽기 중 'Git: x'                           | 'repeat'      |
 * | 위 뒤 다음 다시 읽기 중 'Git: y'                           | 'first'       |
 * | 오류 없이 끝난 다시 읽기 뒤 다시 읽기 중 'Git: x'           | 'first'       |
 */
class BackgroundGitRefresh {
    constructor() {
        this.n_pending = 0;
        this.n_errors = 0;
        // -- 지금 이어지는 실패 묶음: 첫 알림 문구와 띄우지 않은 반복 횟수. 실패 중이 아니면 null·0 --
        this.failure = null;
        this.n_repeats = 0;
    }

    /**
     * `refresh`(백그라운드 다시 읽기 한 번)를 실행한다. 실행 중에 들어온 오류는 `classify`가 실패 묶음으로 센다.
     *
     * Returns
     * -------
     * { failure: string, n_repeats: number } | null
     *     이번 다시 읽기가 오류 없이 끝나 실패 묶음이 닫혔으면 그 묶음(첫 문구·띄우지 않은 반복 횟수), 아니면 null.
     *     `refresh`가 거부해도 오류 알림이 없었으면 성공으로 본다(Git 확장은 실패를 거부가 아니라 알림으로 알린다).
     */
    async run(
        refresh,
    ) {
        const n_errorsBefore = this.n_errors;
        this.n_pending += 1;
        try {
            await refresh();
        } catch {
            // 실패 여부는 오류 알림으로 판정한다.
        } finally {
            this.n_pending -= 1;
        }
        let recovered = null;
        if (this.n_errors === n_errorsBefore && this.failure !== null) {
            recovered = { failure: this.failure, n_repeats: this.n_repeats };
            this.failure = null;
            this.n_repeats = 0;
        }
        return recovered;
    }

    /**
     * 확장이 띄우려는 오류 문구를 분류한다.
     *
     * Returns
     * -------
     * 'foreground' | 'first' | 'repeat'
     *     'foreground'는 백그라운드 다시 읽기와 무관(그대로 띄운다), 'first'는 실패 묶음의 첫 알림(띄운다),
     *     'repeat'는 같은 실패의 반복(띄우지 않는다).
     */
    classify(
        text,
    ) {
        let kind = 'foreground';
        if (this.n_pending > 0) {
            this.n_errors += 1;
            if (text === this.failure) {
                this.n_repeats += 1;
                kind = 'repeat';
            } else {
                this.failure = text;
                this.n_repeats = 0;
                kind = 'first';
            }
        }
        return kind;
    }
}

module.exports = { BackgroundGitRefresh };
