#!/bin/bash
# WSL git이 index·pack을 계속 바꾸는 동안 Windows 쪽 git이 \\wsl.localhost 경로로 status/log를 실행해
# "fatal: mmap: could not determine filesize"·"bad object HEAD" 오류가 나는지 센다. 수동 실행용이며 WSL 안에서 돌린다.
#
# 사용: repro-git-wsl-race.sh <임시 저장소> [Windows 쪽 실행 횟수=30] [시험할 git.exe=Windows git]
#   시험할 git.exe에 Paddock 분배기(paddock-shell/bin/git.exe)를 주면 같은 부하에서 오류가 0인지 비교할 수 있다.
# 주의: 저장소 안에 파일을 만들고 커밋·repack을 반복하므로 `git init`으로 새로 만든 임시 저장소에서만 쓴다.
#   원격이 있는 저장소는 거절한다.
# 기준값(2026-10-14, Windows git 2.55): 60회 중 mmap 3회·bad object 28회.
set -u
REPO="$1"; N_WIN="${2:-30}"; GIT_UNDER_TEST="${3:-/mnt/c/Program Files/Git/cmd/git.exe}"
cd "$REPO" || exit 1
if [ -n "$(git remote)" ]; then echo "원격이 있는 저장소에는 쓰지 않습니다: $REPO" >&2; exit 2; fi
rm -f err.txt stop
WIN=$(wslpath -w "$PWD")
( n=0; while [ ! -f stop ]; do n=$((n+1)); echo $n > f$((n%20)).txt; git add f$((n%20)).txt 2>/dev/null
    if [ $((n%5)) -eq 0 ]; then git -c user.email=a@b -c user.name=t commit -q -m c$n 2>/dev/null; git repack -adq 2>/dev/null; fi
  done; echo "wsl loop iterations: $n" ) &
LOOP=$!
sleep 1
for i in $(seq 1 "$N_WIN"); do
  "$GIT_UNDER_TEST" -C "$WIN" -c safe.directory='*' status --porcelain >/dev/null 2>>err.txt
  "$GIT_UNDER_TEST" -C "$WIN" -c safe.directory='*' log -1 --format=%H >/dev/null 2>>err.txt
done
touch stop; wait $LOOP
echo "--- git runs: $((N_WIN*2)); stderr lines:"; sort err.txt | uniq -c | sort -rn | head
echo "mmap: $(grep -c 'mmap' err.txt)  bad object: $(grep -c 'bad object' err.txt)"
