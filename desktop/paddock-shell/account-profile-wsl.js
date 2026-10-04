/** WSL prepares account folders itself so Linux links and permissions keep their native meaning. */
const { execFile } = require('node:child_process');

class WslScripts {
    static IDENTITY = 'set -eu; printf "%s\\0%s\\0" "$HOME" "${WSL_DISTRO_NAME:-}"';
    // A fixed Python program pins each Linux directory before fchmod; it never follows skill links or reads CLI files.
    static ACCESS = `set -eu
case "$HOME" in /*) ;; *) exit 2 ;; esac
if ! command -v python3 >/dev/null 2>&1; then
    if [ ! -x "$HOME" ]; then
        printf 'settings\\0%s\\0python-unavailable\\0' "$HOME"
        exit 0
    fi
    profile_root="$HOME/.paddock/agent-profiles"
    set -- "$HOME/.paddock" "$profile_root" "\${2:+$profile_root/$2}" "\${2:+$profile_root/$2/$3}" "\${2:+$profile_root/$2/$3/skills}" "\${2:+$profile_root/$2/$3/paddock-bin}"
    for directory do
        [ -n "$directory" ] || continue
        if [ -L "$directory" ] || { [ -e "$directory" ] && [ ! -d "$directory" ]; }; then
            printf 'blocked\\0%s\\0' "$directory"
            exit 0
        fi
        if [ -d "$directory" ] && { [ ! -r "$directory" ] || [ ! -w "$directory" ] || [ ! -x "$directory" ]; }; then
            printf 'settings\\0%s\\0python-unavailable\\0' "$directory"
            exit 0
        fi
    done
    printf 'ready\\0'
    exit 0
fi
exec python3 - "$@" <<'PY'
import os, stat, sys
action, account_id, provider = sys.argv[1:]
def pin_directory(directory):
    flags = os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW
    descriptor = os.open('/', flags)
    try:
        for segment in directory.split('/'):
            if segment:
                next_descriptor = os.open(segment, flags, dir_fd=descriptor)
                os.close(descriptor)
                descriptor = next_descriptor
    except OSError:
        os.close(descriptor)
        raise
    return descriptor

home = os.environ['HOME']
root = os.path.join(home, '.paddock')
directories = [root, os.path.join(root, 'agent-profiles')]
if account_id:
    account = os.path.join(directories[-1], account_id)
    config = os.path.join(account, provider)
    directories.extend([account, config, os.path.join(config, 'skills'), os.path.join(config, 'paddock-bin')])
managed = set(directories)
visited = set()
status, blocked = 'ready', ''
for target in directories:
    ancestors = []
    current = target
    while current != os.path.dirname(current):
        ancestors.insert(0, current)
        current = os.path.dirname(current)
    for directory in ancestors:
        if directory in visited or status != 'ready':
            continue
        visited.add(directory)
        try:
            info = os.lstat(directory)
            if not stat.S_ISDIR(info.st_mode):
                status, blocked = 'blocked', directory
            elif directory in managed and info.st_uid != os.getuid():
                status, blocked = 'settings', directory
            elif directory in managed and stat.S_IMODE(info.st_mode) & 0o700 != 0o700:
                if action == 'repair':
                    descriptor = pin_directory(directory)
                    try:
                        pinned = os.fstat(descriptor)
                        if (pinned.st_dev, pinned.st_ino, pinned.st_uid) != (info.st_dev, info.st_ino, os.getuid()):
                            raise PermissionError('Folder changed')
                        os.chmod('/proc/self/fd/' + str(descriptor), 0o700)
                    finally:
                        os.close(descriptor)
                else:
                    status, blocked = 'repair', directory
            elif not os.access(directory, os.R_OK | os.W_OK | os.X_OK if directory in managed else os.X_OK):
                status, blocked = 'settings', directory
        except FileNotFoundError:
            pass
        except OSError:
            status, blocked = 'settings', directory
sys.stdout.write(status + '\\0' + blocked + '\\0')
PY`;
    static PREPARE = `set -eu
umask 077
profile_id=$1
provider=$2
browser_name=$3
browser_script=$4
case "$HOME" in /*) ;; *) exit 2 ;; esac
profile_root="$HOME/.paddock/agent-profiles"
config_dir="$profile_root/$profile_id/$provider"
for directory in "$HOME/.paddock" "$profile_root" "$profile_root/$profile_id" "$config_dir"; do
    if [ -L "$directory" ] || { [ -e "$directory" ] && [ ! -d "$directory" ]; }; then exit 3; fi
    if [ -d "$directory" ] && { [ ! -r "$directory" ] || [ ! -w "$directory" ] || [ ! -x "$directory" ]; }; then exit 4; fi
    mkdir -p "$directory"
done
chmod 700 "$profile_root" "$profile_root/$profile_id" "$config_dir"
browser_dir="$config_dir/paddock-bin"
if [ -L "$browser_dir" ] || { [ -e "$browser_dir" ] && [ ! -d "$browser_dir" ]; }; then exit 3; fi
if [ -d "$browser_dir" ] && { [ ! -r "$browser_dir" ] || [ ! -w "$browser_dir" ] || [ ! -x "$browser_dir" ]; }; then exit 4; fi
mkdir -p "$browser_dir"
chmod 700 "$browser_dir"
browser_file="$browser_dir/$browser_name"
if [ -L "$browser_file" ] || { [ -e "$browser_file" ] && [ ! -f "$browser_file" ]; }; then exit 3; fi
browser_temp=$(mktemp "$browser_dir/.browser.XXXXXX")
trap 'rm -f "$browser_temp"' EXIT
printf '%s' "$browser_script" > "$browser_temp"
chmod 700 "$browser_temp"
mv -f "$browser_temp" "$browser_file"
trap - EXIT
printf '%s\\0%s\\0' "$config_dir" "$browser_dir"
case "$provider" in
    claude) source_dir="\${CLAUDE_CONFIG_DIR:-$HOME/.claude}/skills" ;;
    codex) source_dir="\${CODEX_HOME:-$HOME/.codex}/skills" ;;
    *) exit 2 ;;
esac
target_dir="$config_dir/skills"
if [ -L "$target_dir" ] || { [ -e "$target_dir" ] && [ ! -d "$target_dir" ]; }; then exit 3; fi
if [ -d "$target_dir" ] && { [ ! -r "$target_dir" ] || [ ! -w "$target_dir" ] || [ ! -x "$target_dir" ]; }; then exit 4; fi
mkdir -p "$target_dir"
chmod 700 "$target_dir"
if [ -d "$source_dir" ]; then
    for source_skill in "$source_dir"/*; do
        skill_name=\${source_skill##*/}
        if [ "$skill_name" != synced ] && [ -d "$source_skill" ] && [ -f "$source_skill/SKILL.md" ]; then
            target_skill="$target_dir/$skill_name"
            if [ -e "$target_skill" ] || [ -L "$target_skill" ]; then
                if [ ! -L "$target_skill" ] || [ "$(readlink "$target_skill")" != "$source_skill" ]; then
                    printf 'An existing skill named %s was kept.\\0' "$skill_name"
                fi
            else
                ln -s "$source_skill" "$target_skill"
            fi
        fi
    done
fi`;
}

/** Runs only fixed scripts; account labels and frontend paths never become shell text. */
async function runWsl(
    script,
    argumentsList,
    distribution,
) {
    const commandArguments = distribution ? ['--distribution', distribution] : [];
    commandArguments.push('--exec', 'sh', '-c', script, 'paddock-accounts', ...argumentsList);
    const output = await new Promise((resolve, reject) => {
        execFile('wsl.exe', commandArguments, { encoding: 'utf8', timeout: 20000, maxBuffer: 1024 * 1024, windowsHide: true }, (error, stdout) => {
            if (error) reject(new Error('The WSL account environment could not be prepared. Check that the distribution is available and its home folder is writable.'));
            else resolve(stdout);
        });
    });
    return output.split('\0').filter(Boolean);
}

module.exports = { WslScripts, runWsl };
