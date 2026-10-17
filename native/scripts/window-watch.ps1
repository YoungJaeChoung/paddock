# 사용: window-watch.ps1 -DurationSec <초> -OutFile <결과 파일>
# 지정한 시간 동안 40 ms마다 화면의 최상위 창을 훑어, 시작 때 없던 콘솔 창(ConsoleWindowClass·Windows Terminal 호스트)이 나타났는지 기록한다.
# git·wsl.exe가 콘솔 창을 만들면(깜빡임) 여기에 한 줄로 남는다. 키·마우스 입력은 보내지 않고 창을 건드리지 않는다.
param(
    [int]$DurationSec = 60,
    [string]$OutFile
)
Add-Type @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public class WatchWin {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  public static List<string> Consoles() {
    var list = new List<string>();
    EnumWindows((h, l) => {
      if (IsWindowVisible(h)) {
        var s = new StringBuilder(256); GetClassName(h, s, 256);
        string c = s.ToString();
        if (c == "ConsoleWindowClass" || c == "CASCADIA_HOSTING_WINDOW_CLASS" || c.StartsWith("PseudoConsole")) { uint p; GetWindowThreadProcessId(h, out p); list.Add(h.ToInt64() + "|" + c + "|" + p); }
      }
      return true; }, IntPtr.Zero);
    return list;
  }
}
"@
$before = @{}
foreach ($item in [WatchWin]::Consoles()) { $before[$item.Split('|')[0]] = $true }
$new = @{}
$samples = 0
$end = (Get-Date).AddSeconds($DurationSec)
while ((Get-Date) -lt $end) {
    $samples++
    foreach ($item in [WatchWin]::Consoles()) {
        $parts = $item.Split('|')
        if (-not $before.ContainsKey($parts[0]) -and -not $new.ContainsKey($parts[0])) {
            $name = (Get-Process -Id ([int]$parts[2]) -ErrorAction SilentlyContinue).ProcessName
            $new[$parts[0]] = "{0} new console window hwnd={1} class={2} process={3}" -f (Get-Date -Format HH:mm:ss.fff), $parts[0], $parts[1], $name
        }
    }
    Start-Sleep -Milliseconds 40
}
$lines = @("samples=$samples new_console_windows=$($new.Count)") + @($new.Values)
$lines | Set-Content $OutFile -Encoding utf8
