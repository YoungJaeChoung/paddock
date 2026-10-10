# Building Paddock from source

Node.js 22 or later and a native build toolchain: Xcode Command Line Tools on macOS (`xcode-select --install`); Visual Studio Build Tools ("Desktop development with C++") and Python 3 on Windows. From the repository root:

```bash
npm run setup
npm run build
npm start                       # or: npm start -- /absolute/path/to/project
```

Linux has no installer yet; run it from source this way.

## Installers

Installers are built on their own system because the terminal's native modules are compiled per OS. Output goes to `desktop/dist/`.

| System | Command | Output |
| --- | --- | --- |
| macOS | `make package-mac` | `Paddock-mac-<arm64 or x64>.dmg` |
| Windows | `make package-win` (or `npm run setup` then `npm run package:win`) | `Paddock-win-x64.exe` |

The **Package** GitHub Actions workflow builds all three installers. A `package/…` branch attaches them to the run as artifacts; `make release` raises the patch version in `package.json`, commits it, pushes `main` and the tag `v<version>`, and the workflow publishes the installers to a new GitHub Release (`make release BUMP=minor` for a minor version). Installed builds and development runs share `~/.paddock/extensions` and `~/.paddock/config`.

The installers aren't code-signed. To check a download, compare its SHA-256 with the one the [release page](https://github.com/YoungJaeChoung/paddock/releases/latest) shows for that file (`shasum -a 256 <file>` on macOS, `Get-FileHash <file>` on Windows).

Product code lives in `desktop/`.

## Current scope

Verified: local terminals, text editing and saving, extension install from Open VSX or a `.vsix` file, and extension editors in file tabs. Not yet: code-signed installers, automatic updates, and screens to update or remove extensions. Not every VS Code extension is compatible.
