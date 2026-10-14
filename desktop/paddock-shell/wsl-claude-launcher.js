/**
 * Puts Paddock's `claude` launcher in front of the Linux `claude` for ordinary WSL terminals opened from the Windows app.
 *
 * The launcher (claude-wrapper.js) adds Paddock's own settings (`--settings`) to a `claude` typed in a terminal. Paddock sets the terminal's
 * PATH for shells it starts itself, but a terminal that runs `wsl.exe` starts the user's own shell inside WSL, whose PATH Paddock cannot change
 * from outside. The only way in is one line in `~/.bashrc` that puts the launcher folder first. Paddock asks once before adding it,
 * marks the line with a comment so it can be found again, never adds it twice, and removes exactly that line when told to.
 *
 * Everything runs inside WSL through one fixed script (no caller text becomes shell text except the launcher file's content, which is
 * written verbatim). When `~/.bashrc` is a link (for example into a dotfiles repository) the real file is edited and the link stays.
 */
const { runWsl } = require('./account-profile-wsl');
const { claudeWrapperScript } = require('./claude-wrapper');

class WslClaudeLauncher {
    // ~/.bashrc의 이 줄을 알아보는 표지. 이 표지가 든 줄만 Paddock이 넣은 것으로 본다.
    static MARKER = '# paddock-claude-wrapper';
    // 줄 끝 상태는 첫 값: 'installed'(실행 파일 놓음), 'added'·'present'(줄을 넣음·이미 있음), 'removed'·'absent'(줄을 뺌·원래 없음), 'blocked'(.bashrc가 끊어진 링크 등).
    static SCRIPT = String.raw`set -eu
case "$HOME" in /*) ;; *) exit 2 ;; esac
action=$1
marker='# paddock-claude-wrapper'
case "$action" in
install)
    wrapper_text=$2
    bin_dir="$HOME/.paddock/config/bin"
    mkdir -p "$bin_dir"
    wrapper_file="$bin_dir/claude"
    if [ -L "$wrapper_file" ] || { [ -e "$wrapper_file" ] && [ ! -f "$wrapper_file" ]; }; then exit 3; fi
    wrapper_temp=$(mktemp "$bin_dir/.claude.XXXXXX")
    trap 'rm -f "$wrapper_temp"' EXIT
    printf '%s' "$wrapper_text" > "$wrapper_temp"
    chmod 700 "$wrapper_temp"
    mv -f "$wrapper_temp" "$wrapper_file"
    trap - EXIT
    printf 'installed\0'
    ;;
add|remove)
    rc="$HOME/.bashrc"
    rc_real="$rc"
    is_link=no
    state=""
    if [ -L "$rc" ]; then
        is_link=yes
        rc_real=$(readlink -f "$rc" || true)
        if [ -z "$rc_real" ] || [ ! -f "$rc_real" ]; then state=blocked; fi
    elif [ -e "$rc" ] && [ ! -f "$rc" ]; then
        state=blocked
    fi
    if [ -z "$state" ]; then
        present=no
        if [ -f "$rc_real" ] && grep -qF -- "$marker" "$rc_real"; then present=yes; fi
        if [ "$action" = add ]; then
            if [ "$present" = yes ]; then
                state=present
            else
                if [ -s "$rc_real" ] && [ -n "$(tail -c 1 "$rc_real")" ]; then printf '\n' >> "$rc_real"; fi
                printf '%s %s\n' 'export PATH="$HOME/.paddock/config/bin:$PATH"' "$marker" >> "$rc_real"
                state=added
            fi
        elif [ "$present" = yes ]; then
            rc_temp=$(mktemp)
            trap 'rm -f "$rc_temp"' EXIT
            grep -vF -- "$marker" "$rc_real" > "$rc_temp" || true
            cat "$rc_temp" > "$rc_real"
            rm -f "$rc_temp"
            trap - EXIT
            state=removed
        else
            state=absent
        fi
    fi
    printf '%s\0%s\0%s\0' "$state" "$rc_real" "$is_link"
    ;;
*)
    exit 2
    ;;
esac
`;
}

/**
 * Interprets the fixed script's output: `{ state, file, isLinked }` (`file` and `isLinked` are only reported for the `.bashrc` actions).
 *
 * Examples
 * --------
 * | output items                                          | result                                                         |
 * | ----------------------------------------------------- | -------------------------------------------------------------- |
 * | `['installed']`                                       | `{ state: 'installed', file: '', isLinked: false }`            |
 * | `['added', '/home/me/.bashrc', 'no']`                 | `{ state: 'added', file: '/home/me/.bashrc', isLinked: false }` |
 * | `['present', '/repo/bashrc', 'yes']`                  | `{ state: 'present', file: '/repo/bashrc', isLinked: true }`   |
 */
function parseLauncherOutput(
    items,
) {
    return { state: items[0] || 'unsupported', file: items[1] || '', isLinked: items[2] === 'yes' };
}

/** Puts the launcher file in `~/.paddock/config/bin` of the default WSL distribution. `flagFile` is the Paddock settings file path as WSL sees it. */
async function installWslLauncher(
    flagFile,
    distribution = '',
) {
    return parseLauncherOutput(await runWsl(WslClaudeLauncher.SCRIPT, ['install', claudeWrapperScript({ kind: 'sh', flagFile })], distribution));
}

/** Adds (`true`) or removes (`false`) the PATH line in the default WSL distribution's `~/.bashrc`. */
async function setWslLauncherPath(
    isEnabled,
    distribution = '',
) {
    return parseLauncherOutput(await runWsl(WslClaudeLauncher.SCRIPT, [isEnabled ? 'add' : 'remove'], distribution));
}

module.exports = { WslClaudeLauncher, parseLauncherOutput, installWslLauncher, setWslLauncherPath };
