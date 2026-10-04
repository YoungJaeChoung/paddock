/** WSL prepares account folders itself so Linux links and permissions keep their native meaning. */
const { execFile } = require('node:child_process');

class WslScripts {
    static IDENTITY = 'set -eu; printf "%s\\0%s\\0" "$HOME" "${WSL_DISTRO_NAME:-}"';
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
    mkdir -p "$directory"
done
chmod 700 "$profile_root" "$profile_root/$profile_id" "$config_dir"
browser_dir="$config_dir/paddock-bin"
if [ -L "$browser_dir" ] || { [ -e "$browser_dir" ] && [ ! -d "$browser_dir" ]; }; then exit 3; fi
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
