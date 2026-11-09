#!/bin/bash
# 시험 자료(사용량): 임시 홈 $1 아래에 가짜 사용량 기록과 가짜 계정·가짜 codex를 만든다. 실제 ~/.paddock는 건드리지 않는다.
#   - 기본 Claude 기록(config/usage/claude.json): 5h 23% · week 61%, 켜짐(state.json)
#   - 등록 Claude 계정 Main: 자기 폴더의 paddock-usage/claude.json 5h 12% · week 34%
#   - 가짜 claude.exe(PING 복사): `claude.exe -n 600 127.0.0.1`로 프로그램 이름 claude를 흉내 낸다(앱이 설정 폴더 bin에 놓는 claude 래퍼가 PATH 앞에 있어 `.exe`를 붙여 래퍼를 건너뛴다)
#   - 가짜 codex.cmd: `codex app-server`가 initialize와 한도 읽기에 답한다(5h 40%). 환경 변수 FAKE_CODEX_MODE=silent이면 답하지 않는다
#   - state.json 시드: 지난 실행에서 Codex 기본 CLI를 골랐던 것으로 시작한다
H=$1
now=$(date +%s); five=$((now + 5 * 3600)); week=$((now + 3 * 86400))
ID=11111111-1111-4111-8111-111111111111
C=$H/.paddock/config
mkdir -p $H/.codex $C/usage $C/agent-profiles/$ID/claude/paddock-usage $H/bin $H/.paddock/native
printf '{"rate_limits":{"five_hour":{"used_percentage":23,"resets_at":%s},"seven_day":{"used_percentage":61,"resets_at":%s}},"updated_at":%s}\n' $five $week $now > $C/usage/claude.json
printf '{"claude":"on"}\n' > $C/usage/state.json
printf '{"rate_limits":{"five_hour":{"used_percentage":12,"resets_at":%s},"seven_day":{"used_percentage":34,"resets_at":%s}},"updated_at":%s}\n' $five $week $now > $C/agent-profiles/$ID/claude/paddock-usage/claude.json
printf '{\n  "version": 1,\n  "profiles": [\n    {"id": "%s", "provider": "claude", "label": "Main", "runtime": "native"}\n  ]\n}\n' $ID > $C/account-profiles.json
cp /mnt/c/Windows/System32/PING.EXE $H/bin/claude.exe
printf '@echo off\r\npowershell -NoProfile -ExecutionPolicy Bypass -File "%%~dp0codex-fake.ps1" %%*\r\n' > $H/bin/codex.cmd
cat > $H/bin/codex-fake.ps1 <<PS
if (\$args[0] -ne 'app-server') { exit 2 }
\$reset = $five
while (\$true) {
  \$line = [Console]::In.ReadLine()
  if (\$line -eq \$null) { break }
  if (\$env:FAKE_CODEX_MODE -eq 'silent') { continue }
  if (\$line -like '*"initialize"*') { [Console]::Out.WriteLine('{"id":1,"result":{}}'); [Console]::Out.Flush() }
  elseif (\$line -like '*rateLimits/read*') { [Console]::Out.WriteLine('{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":40,"windowDurationMins":300,"resetsAt":' + \$reset + '}}}}'); [Console]::Out.Flush() }
}
PS
printf '{"version":3,"daemon":"seed","usage_selection":{"codex":{"provider":"codex"}}}\n' > $H/.paddock/native/state.json
