#!/bin/bash
# 시험 자료(사용량 목록): 임시 홈 $1에 Claude 계정 7개와 기본 Claude 로그인(첫 계정과 같음)을 만든다. 모두 가짜이고 실제 ~/.paddock는 건드리지 않는다.
#   계정 1 Alpha One(로그인 login-1) · 2 Beta Two · 3 Wolf Three · 4 Work Four · 5 Echo Five · 6 Fox Six · 7 Golf Seven(각자 login-N)
#   기본 Claude: config/usage/claude.json(5h 23) + 홈의 .claude.json 로그인 login-1 → 기본 CLI는 Alpha One으로 연결된다
#   stage/ : 시험이 `Copy-Item`으로 바꿔 넣는 파일들 — profiles-8.json(같은 로그인 login-1의 Alpha Twin 추가), broken.json(깨진 기록), good.json(Beta Two의 정상 기록)
H=$1
now=$(date +%s); five=$((now + 5 * 3600)); week=$((now + 3 * 86400))
C=$H/.paddock/config
names=("Alpha One" "Beta Two" "Wolf Three" "Work Four" "Echo Five" "Fox Six" "Golf Seven")
id() { printf 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa%s' $1; }
mkdir -p $C/usage $H/bin $H/stage $H/.paddock/native
printf '{"claude":"on"}\n' > $C/usage/state.json
printf '{"rate_limits":{"five_hour":{"used_percentage":23,"resets_at":%s},"seven_day":{"used_percentage":61,"resets_at":%s}},"updated_at":%s}\n' $five $week $now > $C/usage/claude.json
printf '{"oauthAccount":{"accountUuid":"login-1"}}\n' > $H/.claude.json
items=""
for i in 1 2 3 4 5 6 7; do
  d=$C/agent-profiles/$(id $i)/claude
  mkdir -p $d/paddock-usage
  printf '{"rate_limits":{"five_hour":{"used_percentage":%s,"resets_at":%s},"seven_day":{"used_percentage":%s,"resets_at":%s}},"updated_at":%s}\n' $((10 + i)) $five $((30 + i)) $week $now > $d/paddock-usage/claude.json
  printf '{"oauthAccount":{"accountUuid":"login-%s"}}\n' $i > $d/.claude.json
  items="$items{\"id\": \"$(id $i)\", \"provider\": \"claude\", \"label\": \"${names[$((i-1))]}\", \"runtime\": \"native\"},"
done
printf '{"version": 1, "profiles": [%s]}\n' "${items%,}" > $C/account-profiles.json
twin=$(id 8); mkdir -p $C/agent-profiles/$twin/claude/paddock-usage
printf '{"oauthAccount":{"accountUuid":"login-1"}}\n' > $C/agent-profiles/$twin/claude/.claude.json
printf '{"version": 1, "profiles": [%s,{"id": "%s", "provider": "claude", "label": "Alpha Twin", "runtime": "native"}]}\n' "${items%,}" $twin > $H/stage/profiles-8.json
printf '{unfinished' > $H/stage/broken.json
cp $C/agent-profiles/$(id 2)/claude/paddock-usage/claude.json $H/stage/good.json
cp /mnt/c/Windows/System32/PING.EXE $H/bin/claude.exe
