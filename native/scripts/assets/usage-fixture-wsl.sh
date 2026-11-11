#!/bin/bash
# 시험 자료(WSL 사용량): 임시 홈 $1의 계정 목록에 WSL 계정 하나를 등록하고 상태 줄 선택으로 시드한다. 계정 id는 실제 WSL 홈에 이미 있는 Claude 계정 폴더의 id다 —
# 앱은 그 폴더의 paddock-usage/claude.json을 \\wsl.localhost 경로로 읽기만 하고(쓰지 않는다), 시험은 같은 파일을 따로 읽어 값을 견준다.
H=$1
ID=${PDK_WSL_ACCOUNT:-adcb6b8f-a6d8-4880-a8b8-51559e801174}
C=$H/.paddock/config
mkdir -p $C $H/.paddock/native $H/bin
printf '{\n  "version": 1,\n  "profiles": [\n    {"id": "%s", "provider": "claude", "label": "WSL Real", "runtime": "wsl", "wslDistribution": "Ubuntu"}\n  ]\n}\n' $ID > $C/account-profiles.json
printf '{"version":3,"daemon":"seed","usage_selection":{"claude":{"provider":"claude","profile":{"id":"%s","provider":"claude","label":"WSL Real","runtime":"wsl","wslDistribution":"Ubuntu"}}}}\n' $ID > $H/.paddock/native/state.json
