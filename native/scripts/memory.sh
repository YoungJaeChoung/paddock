#!/usr/bin/env bash
# 사용: memory.sh [--check]
# 지금 떠 있는 paddock-app·paddock-daemon의 Private 메모리(MB)를 /proc/<pid>/smaps_rollup 에서 읽어 보여 준다.
# Private = Private_Clean + Private_Dirty. 공유 라이브러리 몫은 빼므로 Windows의 Private 값과 같은 성격의 참고치다.
# 셸은 세지 않는다(프로세스 이름이 paddock-app·paddock-daemon인 것만). --check이면 예산(앱 ≤ 180 · 데몬 ≤ 40 · 합 ≤ 220)을 넘을 때 종료 코드 1.
# 시나리오(터미널 4개·5만 줄)는 Windows의 memory.ps1이 돌린다 — Linux에서는 GUI 앱이 뜨지 않아 지금 떠 있는 프로세스만 잰다.
set -euo pipefail

budget_app=180
budget_daemon=40
budget_total=220

# 프로세스 이름 하나의 Private MB 합.
private_mb() {
    local name="$1" total_kb=0 pid kb
    for pid in $(pgrep -x "$name" || true); do
        kb=$(awk '/^Private_(Clean|Dirty):/ { sum += $2 } END { print sum + 0 }' "/proc/$pid/smaps_rollup" 2>/dev/null || echo 0)
        total_kb=$((total_kb + kb))
    done
    awk -v kb="$total_kb" 'BEGIN { printf "%.1f", kb / 1024 }'
}

app_mb=$(private_mb paddock-app)
daemon_mb=$(private_mb paddock-daemon)
total_mb=$(awk -v a="$app_mb" -v d="$daemon_mb" 'BEGIN { printf "%.1f", a + d }')
echo "app=${app_mb} MB  daemon=${daemon_mb} MB  total=${total_mb} MB"

status=0
if [ "${1:-}" = "--check" ]; then
    awk -v a="$app_mb" -v d="$daemon_mb" -v t="$total_mb" -v ba="$budget_app" -v bd="$budget_daemon" -v bt="$budget_total" \
        'BEGIN { exit !(a <= ba && d <= bd && t <= bt) }' || status=1
    if [ "$status" -eq 0 ]; then echo PASS; else echo FAIL; fi
fi
exit "$status"
