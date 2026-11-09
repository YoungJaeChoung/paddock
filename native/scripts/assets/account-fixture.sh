#!/bin/bash
# 시험 자료(계정 터미널): 임시 홈 $1에 등록 계정 둘(Claude · Main, Codex · Work)과, 실행 환경을 한 줄로 찍는 가짜 claude·codex 명령을 만든다.
# 실제 ~/.paddock는 건드리지 않는다. 가짜 명령은 `ACCOUNT_FIXTURE|CLAUDE_CONFIG_DIR=…|KEY=[ANTHROPIC_API_KEY]|ARGS=…` 꼴로 찍어
# 계정 터미널이 계정 폴더 변수를 정하고 로그인을 우회하는 변수를 비우는지 대본이 읽게 한다. 대본: account-terminal.script·memory-account.script.
H=$1
C=$H/.paddock/config
CLAUDE_ID=7f0c2a3e-4b1d-4c8e-9a5f-0123456789ab
CODEX_ID=0a1b2c3d-4e5f-4a6b-8c7d-89abcdef0123
mkdir -p $C $H/bin $C/agent-profiles/$CLAUDE_ID/claude $C/agent-profiles/$CODEX_ID/codex
printf '{\n  "version": 1,\n  "profiles": [\n    {"id": "%s", "provider": "claude", "label": "Main", "runtime": "native"},\n    {"id": "%s", "provider": "codex", "label": "Work", "runtime": "native"}\n  ]\n}\n' $CLAUDE_ID $CODEX_ID > $C/account-profiles.json
printf '@echo off\r\necho ACCOUNT_FIXTURE^|CLAUDE_CONFIG_DIR=%%CLAUDE_CONFIG_DIR%%^|KEY=[%%ANTHROPIC_API_KEY%%]^|ARGS=%%*\r\n' > $H/bin/claude.cmd
printf '@echo off\r\necho ACCOUNT_FIXTURE^|CODEX_HOME=%%CODEX_HOME%%^|KEY=[%%OPENAI_API_KEY%%]^|ARGS=%%*\r\n' > $H/bin/codex.cmd
