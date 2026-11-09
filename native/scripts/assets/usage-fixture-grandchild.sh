#!/bin/bash
# 가짜 codex: PowerShell이 손자(`ping -n 601`)를 띄우고 답한 뒤, 입력이 닫혀도 손자가 끝나기를 기다린다.
bash /home/youngjae/paddock-evidence/native/stage4/chunk29/prep-usage.sh $1
cat > $1/bin/codex-fake.ps1 <<'PS'
if ($args[0] -ne 'app-server') { exit 2 }
$grandchild = Start-Process -FilePath ping -ArgumentList '-n', '601', '127.0.0.1' -PassThru -WindowStyle Hidden
while ($true) {
  $line = [Console]::In.ReadLine()
  if ($line -eq $null) { break }
  if ($line -like '*"initialize"*') { [Console]::Out.WriteLine('{"id":1,"result":{}}'); [Console]::Out.Flush() }
  elseif ($line -like '*rateLimits/read*') { [Console]::Out.WriteLine('{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":40,"windowDurationMins":300,"resetsAt":1999999999}}}}'); [Console]::Out.Flush() }
}
$grandchild.WaitForExit()
PS
