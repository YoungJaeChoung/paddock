#!/bin/sh
# 메모리가 부족할 때만 에이전트에게 알리는 PreToolUse 훅(Claude Code).
#
# 에이전트가 메모리를 쓰는 도구(Bash·Agent·Task)를 쓰기 직전에 WSL의 가용 메모리를 읽는다.
# 가용 메모리가 기준(기본 4096MB) 아래면 그 호출의 문맥에 안내 한 문장을 붙이고, 아니면 아무것도 출력하지 않는다.
# 실행을 막거나 프로세스를 종료하지 않는다. 부족한 동안은 해당 도구를 쓸 때마다 붙는다.
#
# 입력: Claude Code가 stdin으로 주는 훅 JSON(tool_name 포함).
# 출력: 부족하면 `{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"…"}}` 한 줄, 아니면 없음.
# 기준 변경: 환경 변수 PADDOCK_MEMORY_MIN_MB(MB). 0이면 항상 조용하다.
#
# 끄기: 스크립트와 같은 폴더에 `memory-hook.off` 파일이 있으면 아무것도 하지 않는다.
# 설정에서 훅을 지워도 이미 떠 있는 세션이 이 스크립트를 계속 부를 수 있어, 꺼짐을 스크립트가 직접 확인한다.
#
# JSON을 jq 없이 다루려고 tool_name 한 칸만 sed로 읽는다. 내장 명령과 sed만 쓴다.

# 입력은 꺼짐 확인보다 먼저 끝까지 읽는다. 읽기 전에 끝내면 입력을 보내는 쪽이 닫힌 파이프에 쓰다 EPIPE 오류를 낸다.
input=$(cat)

here=${0%/*}
if [ -e "$here/memory-hook.off" ]; then
    exit 0
fi

tool=$(printf '%s' "$input" | sed -n 's/.*"tool_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')

case "$tool" in
    Bash|Agent|Task)
        available_kb=
        while read -r key value _; do
            case $key in MemAvailable:) available_kb=$value;; esac
        done 2>/dev/null < /proc/meminfo

        minimum_mb=${PADDOCK_MEMORY_MIN_MB:-4096}
        case "$available_kb$minimum_mb" in
            *[!0-9]*) ;;
            *)
                available_mb=$((available_kb / 1024))
                if [ "$available_kb" != "" ] && [ "$available_mb" -lt "$minimum_mb" ]; then
                    printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"Memory is low: %s MB available (limit %s MB). Do not start memory-heavy work now (builds, test runners, browsers, other agents). Wait for running work to finish, or tell the user before continuing."}}\n' "$available_mb" "$minimum_mb"
                fi
                ;;
        esac
        ;;
esac
exit 0
