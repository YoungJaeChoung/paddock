/** Opens CLI web links in the Windows default browser without handling login credentials. */
class BrowserBridge {
    static NAME = 'paddock-open-browser';
    // The URL travels through stdin, never through PowerShell source or a saved file.
    // A bare executable name also works when the CLI splits BROWSER on whitespace.
    static SCRIPT = String.raw`#!/bin/sh
set -eu
if [ "$#" -ne 1 ]; then
    printf '%s\n' 'Expected one web address.' >&2
    exit 1
fi
case "$1" in
    https://*|http://*) ;;
    *) printf '%s\n' 'Only web addresses can be opened.' >&2; exit 1 ;;
esac
windows_shell=$(command -v powershell.exe || command -v pwsh.exe || true)
if [ -z "$windows_shell" ] && [ -x /mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe ]; then
    windows_shell=/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe
fi
if [ -z "$windows_shell" ]; then
    printf '%s\n' 'Windows browser could not be opened. Copy the login URL into your Windows browser.' >&2
    exit 1
fi
if ! printf '%s' "$1" | "$windows_shell" -NoLogo -NoProfile -NonInteractive -Command '[Console]::InputEncoding = New-Object System.Text.UTF8Encoding; $ErrorActionPreference = "Stop"; try { $url = [Console]::In.ReadToEnd(); $uri = [Uri]$url; if (-not $uri.IsAbsoluteUri -or $uri.Scheme -notin @("http", "https")) { throw "Unsupported address" }; Start-Process -FilePath $url } catch { exit 1 }' >/dev/null 2>&1; then
    printf '%s\n' 'Windows browser could not be opened. Copy the login URL into your Windows browser.' >&2
    exit 1
fi
`;
}

module.exports = { BrowserBridge };
