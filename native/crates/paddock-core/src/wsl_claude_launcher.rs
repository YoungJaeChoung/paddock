//! WSL 일반 터미널에서 Paddock의 `claude` 실행 래퍼가 진짜 `claude`보다 앞서게 하는 고정 셸 스크립트와 그 출력 해석.
//!
//! 래퍼(`claude_wrapper`)는 터미널에서 친 `claude`에 Paddock 전용 설정(`--settings`)을 붙인다. Paddock이 직접 띄우는 셸은 PATH를 정할 수
//! 있지만, `wsl.exe`를 실행하는 터미널은 WSL 안에서 사용자의 셸이 시작되어 밖에서 PATH를 바꿀 수 없다. 들어갈 길은 `~/.bashrc`에
//! 래퍼 폴더를 PATH 앞에 두는 한 줄뿐이다. Paddock은 넣기 전에 한 번 묻고, 줄에 주석 표지를 달아 다시 찾을 수 있게 하며, 두 번 넣지
//! 않고, 시킬 때 정확히 그 줄만 뺀다. 모든 일은 WSL 안에서 고정 스크립트 하나로 하고, 호출자의 글은 래퍼 파일 내용(그대로 쓴다)만
//! 셸 글이 된다. `~/.bashrc`가 링크(예: dotfiles 저장소)이면 링크는 두고 실제 파일을 고친다. 기존 앱과 같은 글자여야 하므로 글자 그대로 옮겼다.

/// 고정 스크립트의 마지막 줄 상태는 첫 값이다. `installed`(실행 파일 놓음), `added`·`present`(줄을 넣음·이미 있음),
/// `removed`·`absent`(줄을 뺌·원래 없음), `blocked`(`.bashrc`가 끊어진 링크 등).
pub const SCRIPT: &str = r#"set -eu
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
"#;

/// 고정 스크립트의 결과. `file`·`is_linked`는 `.bashrc` 동작(`add`·`remove`)에서만 채워진다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LauncherOutput {
    pub state: String,
    pub file: String,
    pub is_linked: bool,
}

/// 고정 스크립트의 출력(NUL로 끝나는 항목들)을 해석한다. 출력이 비면 상태는 `unsupported`다.
///
/// Examples
/// --------
/// | 출력 항목                              | 결과                                                    |
/// |----------------------------------------|---------------------------------------------------------|
/// | `installed`                            | `{ installed, "", false }`                              |
/// | `added`·`/home/me/.bashrc`·`no`        | `{ added, "/home/me/.bashrc", false }`                  |
/// | `present`·`/repo/bashrc`·`yes`         | `{ present, "/repo/bashrc", true }`                     |
/// | (없음)                                 | `{ unsupported, "", false }`                            |
pub fn parse_launcher_output(output: &str) -> LauncherOutput {
    let items: Vec<&str> = output.split('\0').collect();
    LauncherOutput {
        state: items.first().filter(|item| !item.is_empty()).map_or("unsupported", |item| item).to_string(),
        file: items.get(1).copied().unwrap_or("").to_string(),
        is_linked: items.get(2) == Some(&"yes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_wl_06_the_output_items_are_read_as_state_file_and_link_flag() {
        assert_eq!(
            parse_launcher_output("installed\0"),
            LauncherOutput { state: "installed".into(), file: String::new(), is_linked: false }
        );
        assert_eq!(
            parse_launcher_output("added\0/home/me/.bashrc\0no\0"),
            LauncherOutput { state: "added".into(), file: "/home/me/.bashrc".into(), is_linked: false }
        );
        assert_eq!(
            parse_launcher_output("present\0/repo/bashrc\0yes\0"),
            LauncherOutput { state: "present".into(), file: "/repo/bashrc".into(), is_linked: true }
        );
        assert_eq!(parse_launcher_output("").state, "unsupported");
    }

    #[cfg(unix)]
    #[test]
    fn test_c_wl_the_fixed_script_has_no_syntax_errors_and_mentions_only_the_marker_it_edits() {
        let output = std::process::Command::new("sh").args(["-n", "-c", SCRIPT]).output().expect("sh runs");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(SCRIPT.contains(crate::claude_wrapper::ClaudeWrapper::BASHRC_MARKER));
    }

    /// 임시 HOME에서 고정 스크립트를 Linux `sh`로 실행한다. 진짜 홈은 건드리지 않는다.
    #[cfg(unix)]
    fn run_in(
        home: &std::path::Path,
        args: &[&str],
    ) -> LauncherOutput {
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(SCRIPT)
            .arg("sh")
            .args(args)
            .env("HOME", home)
            .output()
            .expect("sh runs");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        parse_launcher_output(&String::from_utf8_lossy(&output.stdout))
    }

    #[cfg(unix)]
    #[test]
    fn test_c_wl_01_to_05_the_script_adds_once_removes_exactly_that_line_follows_links_and_stops_at_broken_ones() {
        use std::fs;
        let home = std::env::temp_dir().join(format!("wsl launcher {}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        let rc = home.join(".bashrc");
        fs::write(&rc, "alias a=b").unwrap();
        // 줄 끝 개행이 없어도 새 줄에 넣고, 두 번째 add는 present다.
        assert_eq!(run_in(&home, &["add"]).state, "added");
        assert_eq!(run_in(&home, &["add"]).state, "present");
        let text = fs::read_to_string(&rc).unwrap();
        assert_eq!(text.matches("# paddock-claude-wrapper").count(), 1);
        assert!(text.starts_with("alias a=b\nexport PATH="));
        // remove는 표지 줄만 뺀다.
        assert_eq!(run_in(&home, &["remove"]).state, "removed");
        assert_eq!(fs::read_to_string(&rc).unwrap(), "alias a=b\n");
        assert_eq!(run_in(&home, &["remove"]).state, "absent");
        // 링크이면 실제 파일을 고치고 링크는 남는다.
        let target = home.join("dotfiles-bashrc");
        fs::rename(&rc, &target).unwrap();
        std::os::unix::fs::symlink(&target, &rc).unwrap();
        let linked = run_in(&home, &["add"]);
        assert_eq!((linked.state.as_str(), linked.is_linked), ("added", true));
        assert!(fs::symlink_metadata(&rc).unwrap().file_type().is_symlink());
        assert!(fs::read_to_string(&target).unwrap().contains("# paddock-claude-wrapper"));
        // 끊어진 링크는 건드리지 않는다.
        fs::remove_file(&target).unwrap();
        assert_eq!(run_in(&home, &["add"]).state, "blocked");
        let _ = fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn test_c_wl_install_puts_the_launcher_file_with_owner_only_permission_and_refuses_a_link() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let home = std::env::temp_dir().join(format!("wsl install {}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        assert_eq!(run_in(&home, &["install", "#!/bin/sh\necho hi\n"]).state, "installed");
        let file = home.join(".paddock/config/bin/claude");
        assert_eq!(fs::read_to_string(&file).unwrap(), "#!/bin/sh\necho hi\n");
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o700);
        fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink("/bin/true", &file).unwrap();
        let output = std::process::Command::new("sh")
            .args(["-c", SCRIPT, "sh", "install", "x"])
            .env("HOME", &home)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3));
        let _ = fs::remove_dir_all(&home);
    }
}
