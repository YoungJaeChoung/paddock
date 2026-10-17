# 사용: automation-run.ps1 -Exe <paddock-app.exe> -Script <대본> -Out <결과 폴더> -HomeDir <임시 사용자 폴더> [-TimeoutSec 120]
# 자동 점검 대본(automation 기능으로 빌드한 앱)을 실행하고, 대본이 남기는 요청 파일을 처리한다.
#   shot-NAME.req -> 창 캡처 NAME.png (PrintWindow)
#   mem-NAME.req  -> 앱·데몬의 Private 메모리(MB)를 mem.log에 한 줄
# -Offscreen 이면 창을 화면 밖(x=-3000)에 활성화 없이 띄워 사용자의 화면·포커스를 가리지 않는다(캡처는 PrintWindow라 가려져도 된다).
# 키보드·마우스 입력은 보내지 않는다. -HomeDir의 폴더로 USERPROFILE을 바꿔 실행해
# 사용자의 실제 ~/.paddock/native를 건드리지 않는다. 이 스크립트가 띄운 앱 프로세스만 다룬다.
# 안전 규칙
#   - HomeDir을 생략하거나 실제 사용자 폴더를 주면 거부한다.
#   - 이 HomeDir의 daemon.json 이 가리키는 것이 아닌 paddock-daemon 이 떠 있으면 시작하지 않는다(파이프 이름이 사용자마다 하나라 시험이 섞인다).
param(
    [string]$Exe,
    [string]$Script,
    [string]$Out,
    [string]$HomeDir = "",
    [int]$TimeoutSec = 120,
    [switch]$Offscreen
)
$realHome = [Environment]::GetFolderPath('UserProfile')
if ($HomeDir -eq "") { throw "HomeDir is required (omitting it would run with the real user profile)" }
if ((Resolve-Path -LiteralPath $HomeDir -ErrorAction SilentlyContinue) -and ((Resolve-Path -LiteralPath $HomeDir).Path.TrimEnd('\') -ieq $realHome.TrimEnd('\'))) { throw "HomeDir is the real user profile" }
$ownDaemonPid = 0
$ownInfo = Join-Path $HomeDir '.paddock\native\daemon.json'
if (Test-Path $ownInfo) { try { $ownDaemonPid = [int](Get-Content $ownInfo -Raw | ConvertFrom-Json).pid } catch {} }
$running = @(Get-Process -Name paddock-daemon -ErrorAction SilentlyContinue | Where-Object { $_.Id -ne $ownDaemonPid })
if ($running.Count -gt 0) { throw "paddock-daemon is already running (pid $($running.Id -join ','))" }
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class CaptureNative {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public struct Rect { public int Left, Top, Right, Bottom; }
}
"@
[void][CaptureNative]::SetProcessDPIAware()
New-Item -ItemType Directory -Force $Out | Out-Null
New-Item -ItemType Directory -Force $HomeDir | Out-Null
. (Join-Path $PSScriptRoot 'test-env.ps1')
Use-TestHome $HomeDir
$proc = Start-Process -FilePath $Exe -ArgumentList "--script `"$Script`" --out `"$Out`"$(if ($Offscreen) { ' --offscreen' })" -PassThru
"started pid=$($proc.Id) script=$Script" | Out-File "$Out\runner.log" -Append -Encoding utf8

function Capture($path) {
    $proc.Refresh()
    $handle = $proc.MainWindowHandle
    if ($handle -eq [IntPtr]::Zero) { "no window handle for $path" | Out-File "$Out\runner.log" -Append -Encoding utf8; return }
    $rect = New-Object CaptureNative+Rect
    [void][CaptureNative]::GetWindowRect($handle, [ref]$rect)
    $width = $rect.Right - $rect.Left; $height = $rect.Bottom - $rect.Top
    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap); $dc = $graphics.GetHdc()
    [void][CaptureNative]::PrintWindow($handle, $dc, 2)
    $graphics.ReleaseHdc($dc); $graphics.Dispose()
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $bitmap.Dispose()
    "captured $path ${width}x${height}" | Out-File "$Out\runner.log" -Append -Encoding utf8
}

function Sample-Memory($label) {
    $targets = @($proc.Id)
    $info = Join-Path $HomeDir ".paddock\native\daemon.json"
    if (Test-Path $info) {
        try { $daemonPid = (Get-Content $info -Raw | ConvertFrom-Json).pid; if ($daemonPid) { $targets += $daemonPid } } catch {}
    }
    foreach ($id in $targets) {
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($p) {
            # 그래픽 메모리(드라이버가 이 프로세스 몫으로 잡은 전용·공유 사용량). 카운터를 못 읽는 환경이면 n/a다.
            $gpu = "GpuDedicatedMB=n/a GpuSharedMB=n/a"
            try {
                $ded = (Get-Counter -Counter '\GPU Process Memory(*)\Dedicated Usage' -ErrorAction Stop).CounterSamples | Where-Object { $_.InstanceName -like "pid_${id}_*" } | Measure-Object -Property CookedValue -Sum
                $shr = (Get-Counter -Counter '\GPU Process Memory(*)\Shared Usage' -ErrorAction Stop).CounterSamples | Where-Object { $_.InstanceName -like "pid_${id}_*" } | Measure-Object -Property CookedValue -Sum
                $gpu = ("GpuDedicatedMB={0:N1} GpuSharedMB={1:N1}" -f ($ded.Sum / 1MB), ($shr.Sum / 1MB))
            } catch {}
            ("{0} {1} pid={2} PrivateMB={3:N1} WorkingSetMB={4:N1} {5}" -f $label, $p.ProcessName, $id, ($p.PrivateMemorySize64 / 1MB), ($p.WorkingSet64 / 1MB), $gpu) | Out-File "$Out\mem.log" -Append -Encoding utf8
        }
    }
}

function Sample-Network($label) {
    # 앱 프로세스가 가진 TCP 연결(상대 주소·상태)을 net.log에 남긴다. 연결이 없으면 `none` 한 줄이다.
    $connections = @(Get-NetTCPConnection -OwningProcess $proc.Id -ErrorAction SilentlyContinue)
    if ($connections.Count -eq 0) { "$label none" | Out-File "$Out\net.log" -Append -Encoding utf8 }
    foreach ($c in $connections) { "$label $($c.LocalAddress):$($c.LocalPort) -> $($c.RemoteAddress):$($c.RemotePort) $($c.State)" | Out-File "$Out\net.log" -Append -Encoding utf8 }
}

$started = Get-Date
while (-not $proc.HasExited -and ((Get-Date) - $started).TotalSeconds -lt $TimeoutSec) {
    foreach ($request in Get-ChildItem $Out -Filter *.req -ErrorAction SilentlyContinue) {
        $name = $request.BaseName
        if ($name -like "shot-*") { Capture "$Out\$($name.Substring(5)).png" }
        elseif ($name -like "mem-*") { Sample-Memory $name.Substring(4) }
        elseif ($name -like "net-*") { Sample-Network $name.Substring(4) }
        Rename-Item $request.FullName ($request.FullName + ".done") -Force
    }
    Start-Sleep -Milliseconds 100
}
if (-not $proc.HasExited) {
    "timeout: stopping app pid=$($proc.Id)" | Out-File "$Out\runner.log" -Append -Encoding utf8
    Stop-Process -Id $proc.Id -Force
}
"exit=$($proc.HasExited)" | Out-File "$Out\runner.log" -Append -Encoding utf8
