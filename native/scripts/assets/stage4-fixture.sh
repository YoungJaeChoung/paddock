#!/bin/bash
# 시험 자료(4단계 총측정): prep-stage2와 같은 장면 자료(proj-a·WSL 폴더) 위에 등록 Claude 계정 10개와 사용량 기록을 더한다. 모두 가짜이고 실제 ~/.paddock는 건드리지 않는다.
H=$1
bash "$(dirname "$0")/stage2-fixture.sh" $H > /dev/null
now=$(date +%s); five=$((now + 5 * 3600)); week=$((now + 3 * 86400))
C=$H/.paddock/config
id() { printf 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbb%02d' $1; }
mkdir -p $C/usage
printf '{"claude":"on"}\n' > $C/usage/state.json
printf '{"rate_limits":{"five_hour":{"used_percentage":23,"resets_at":%s},"seven_day":{"used_percentage":61,"resets_at":%s}},"updated_at":%s}\n' $five $week $now > $C/usage/claude.json
items=""
for i in $(seq 1 10); do
  d=$C/agent-profiles/$(id $i)/claude
  mkdir -p $d/paddock-usage
  printf '{"rate_limits":{"five_hour":{"used_percentage":%s,"resets_at":%s}},"updated_at":%s}\n' $((10 + i)) $five $now > $d/paddock-usage/claude.json
  items="$items{\"id\": \"$(id $i)\", \"provider\": \"claude\", \"label\": \"Account $i\", \"runtime\": \"native\"},"
done
printf '{"version": 1, "profiles": [%s]}\n' "${items%,}" > $C/account-profiles.json
