use std::path::PathBuf;
use std::process::Command;

#[tauri::command]
pub fn open_codex_thread(deep_link: String) -> Result<(), String> {
    validate_codex_thread_link(&deep_link)?;
    open_external_url(&deep_link)
}

/// Opens one agent session deep link. Codex and Claude Code are peers here:
/// the scheme selects the validator, and neither side is a fallback for the other.
#[tauri::command]
pub fn open_agent_session(deep_link: String) -> Result<(), String> {
    validate_agent_session_link(&deep_link)?;
    open_external_url(&deep_link)
}

#[tauri::command]
pub fn open_github_source(url: String) -> Result<(), String> {
    validate_github_source_url(&url)?;
    open_external_url(&url)
}

#[tauri::command]
pub fn open_handoff_target(target_id: String) -> Result<(), String> {
    let target = handoff_target(&target_id)?;
    open_native_target(target)
}

#[tauri::command]
pub fn open_codex_handoff(prompt: String, worktree_path: Option<String>) -> Result<(), String> {
    validate_handoff_prompt(&prompt)?;
    let worktree = validate_handoff_worktree_path(worktree_path)?;
    open_external_url(&codex_new_thread_link(&prompt, worktree.as_ref()))
}

#[tauri::command]
pub fn open_claude_handoff(prompt: String, worktree_path: Option<String>) -> Result<(), String> {
    validate_handoff_prompt(&prompt)?;
    let worktree = validate_handoff_worktree_path(worktree_path)?;
    open_external_url(&claude_new_session_link(&prompt, worktree.as_ref()))
}

fn validate_agent_session_link(deep_link: &str) -> Result<(), String> {
    if deep_link.starts_with("codex://") {
        validate_codex_thread_link(deep_link)
    } else if deep_link.starts_with("claude://") {
        validate_claude_session_link(deep_link)
    } else {
        Err("Only codex:// and claude:// agent session links can be opened.".to_string())
    }
}

/// Claude Desktop imports an existing CLI session from `claude://resume?session=<uuid>`.
/// `claude://code/continue` takes a desktop-owned `local_*` id instead, so it cannot
/// stand in for a transcript recorded by the Claude Code lane backend.
fn validate_claude_session_link(deep_link: &str) -> Result<(), String> {
    let session_id = deep_link
        .strip_prefix("claude://resume?session=")
        .ok_or_else(|| "Only claude://resume?session links can be opened.".to_string())?;
    if is_uuid_like(session_id) {
        Ok(())
    } else {
        Err("Claude session link must end with a CLI session UUID.".to_string())
    }
}

fn validate_codex_thread_link(deep_link: &str) -> Result<(), String> {
    let thread_id = deep_link
        .strip_prefix("codex://threads/")
        .ok_or_else(|| "Only codex://threads links can be opened.".to_string())?;
    if is_uuid_like(thread_id) {
        Ok(())
    } else {
        Err("Codex thread link must end with a thread UUID.".to_string())
    }
}

fn validate_github_source_url(url: &str) -> Result<(), String> {
    let path = url
        .strip_prefix("https://github.com/")
        .ok_or_else(|| "Only GitHub source links can be opened.".to_string())?;
    if is_allowed_github_source_path(path) {
        Ok(())
    } else {
        Err("Only GitHub issue, issue comment, and pull request links can be opened.".into())
    }
}

fn is_allowed_github_source_path(path: &str) -> bool {
    let mut parts = path.split('/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(repo), Some("issues"), Some(number))
        | (Some(owner), Some(repo), Some("pull"), Some(number))
            if valid_github_slug(owner) && valid_github_slug(repo) =>
        {
            let number = number.split(['#', '?']).next().unwrap_or_default();
            number.chars().all(|ch| ch.is_ascii_digit()) && !number.is_empty()
        }
        _ => false,
    }
}

fn valid_github_slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn validate_handoff_prompt(prompt: &str) -> Result<(), String> {
    if prompt.trim().is_empty() {
        Err("Handoff prompt cannot be empty.".into())
    } else {
        Ok(())
    }
}

fn validate_handoff_worktree_path(
    worktree_path: Option<String>,
) -> Result<Option<PathBuf>, String> {
    let Some(path) = worktree_path
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    if path.is_absolute() && path.is_dir() {
        Ok(Some(path))
    } else {
        Err("Handoff worktree path must be an existing absolute directory.".into())
    }
}

fn codex_new_thread_link(prompt: &str, worktree_path: Option<&PathBuf>) -> String {
    let mut link = format!(
        "codex://threads/new?prompt={}",
        percent_encode_query(prompt)
    );
    if let Some(path) = worktree_path {
        link.push_str("&path=");
        link.push_str(&percent_encode_query(&path.to_string_lossy()));
    }
    link
}

fn claude_new_session_link(prompt: &str, worktree_path: Option<&PathBuf>) -> String {
    let mut link = format!("claude://code/new?q={}", percent_encode_query(prompt));
    if let Some(path) = worktree_path {
        link.push_str("&folder=");
        link.push_str(&percent_encode_query(&path.to_string_lossy()));
    }
    link
}

fn percent_encode_query(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn handoff_target(target_id: &str) -> Result<HandoffTarget, String> {
    match target_id {
        "codex-app" => Ok(HandoffTarget {
            app_name: "Codex",
            display_name: "Codex App",
        }),
        "claude-code" => Ok(HandoffTarget {
            app_name: "Claude",
            display_name: "Claude",
        }),
        _ => Err("Only configured native handoff targets can be opened.".into()),
    }
}

struct HandoffTarget {
    app_name: &'static str,
    display_name: &'static str,
}

fn is_uuid_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        match index {
            8 | 13 | 18 | 23 => {
                if *byte != b'-' {
                    return false;
                }
            }
            _ => {
                if !byte.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

fn open_external_url(url: &str) -> Result<(), String> {
    let status = platform_open_command(url)
        .status()
        .map_err(|error| format!("Failed to open external link: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("System opener exited with status {status}."))
    }
}

fn open_native_target(target: HandoffTarget) -> Result<(), String> {
    let status = platform_open_native_target_command(target.app_name)
        .status()
        .map_err(|error| format!("Failed to open {}: {error}", target.display_name))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "System opener could not open {} and exited with status {status}.",
            target.display_name
        ))
    }
}

#[cfg(target_os = "macos")]
fn platform_open_command(url: &str) -> Command {
    let mut command = Command::new("open");
    command.arg(url);
    command
}

#[cfg(target_os = "macos")]
fn platform_open_native_target_command(app_name: &str) -> Command {
    let mut command = Command::new("open");
    command.args(["-a", app_name]);
    command
}

#[cfg(target_os = "windows")]
fn platform_open_native_target_command(app_name: &str) -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", "", app_name]);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_open_native_target_command(app_name: &str) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(app_name);
    command
}

#[cfg(target_os = "windows")]
fn platform_open_command(url: &str) -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", "", url]);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_open_command(url: &str) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(url);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_codex_thread_links() {
        assert!(
            validate_codex_thread_link("codex://threads/019e8f37-5cab-74f3-9933-93e3809396e5")
                .is_ok()
        );
    }

    #[test]
    fn rejects_non_thread_links() {
        assert!(validate_codex_thread_link("https://example.com").is_err());
        assert!(validate_codex_thread_link("codex://threads/not-a-thread").is_err());
        assert!(validate_codex_thread_link(
            "codex://threads/019e8f37-5cab-74f3-9933-93e3809396e5/turn/1"
        )
        .is_err());
    }

    #[test]
    fn validates_claude_resume_session_links() {
        assert!(validate_agent_session_link(
            "claude://resume?session=f24747aa-89d6-4c8a-82aa-5028998665f6"
        )
        .is_ok());
        assert!(validate_agent_session_link(
            "codex://threads/019e8f37-5cab-74f3-9933-93e3809396e5"
        )
        .is_ok());
    }

    #[test]
    fn rejects_unsupported_agent_session_links() {
        assert!(validate_agent_session_link("https://example.com").is_err());
        assert!(validate_claude_session_link("claude://resume?session=not-a-session").is_err());
        // A desktop-owned id is a different address space and must not be treated as a CLI session.
        assert!(validate_claude_session_link("claude://code/continue?session=local_abc").is_err());
        assert!(validate_claude_session_link(
            "claude://resume?session=f24747aa-89d6-4c8a-82aa-5028998665f6&source=shea"
        )
        .is_err());
    }

    #[test]
    fn validates_shea_github_source_links() {
        assert!(
            validate_github_source_url("https://github.com/Alive24/shea-symphony/issues/430")
                .is_ok()
        );
        assert!(validate_github_source_url(
            "https://github.com/Alive24/shea-symphony/issues/430#issuecomment-4621294699"
        )
        .is_ok());
        assert!(
            validate_github_source_url("https://github.com/Alive24/shea-symphony/pull/433").is_ok()
        );
    }

    #[test]
    fn validates_target_repo_github_source_links() {
        assert!(validate_github_source_url("https://github.com/other/repo/issues/430").is_ok());
        assert!(validate_github_source_url("https://github.com/other/repo/pull/430").is_ok());
    }

    #[test]
    fn rejects_non_source_github_links() {
        assert!(validate_github_source_url("https://example.com").is_err());
        assert!(
            validate_github_source_url("https://github.com/Alive24/shea-symphony/actions").is_err()
        );
    }

    #[test]
    fn validates_native_handoff_targets() {
        let codex = handoff_target("codex-app").unwrap();
        assert_eq!(codex.app_name, "Codex");
        assert_eq!(codex.display_name, "Codex App");
        let claude = handoff_target("claude-code").unwrap();
        assert_eq!(claude.app_name, "Claude");
        assert_eq!(claude.display_name, "Claude");
        assert!(handoff_target("gemini-cli").is_err());
        assert!(handoff_target("https://example.com").is_err());
    }

    #[test]
    fn validates_codex_handoff_prompt() {
        assert!(validate_handoff_prompt("Use the skill.").is_ok());
        assert!(validate_handoff_prompt("   ").is_err());
    }

    #[test]
    fn validates_codex_handoff_worktree_path() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            validate_handoff_worktree_path(Some(cwd.display().to_string())).unwrap(),
            Some(cwd)
        );
        assert!(validate_handoff_worktree_path(None).unwrap().is_none());
        assert!(validate_handoff_worktree_path(Some("relative/path".into())).is_err());
        assert!(
            validate_handoff_worktree_path(Some("/definitely/missing/shea-worktree".into()))
                .is_err()
        );
    }

    #[test]
    fn builds_claude_new_session_link_with_prompt_and_folder() {
        let path = PathBuf::from("/tmp/shea worktree");
        assert_eq!(
            claude_new_session_link("Review #407\nUse dev.", Some(&path)),
            "claude://code/new?q=Review%20%23407%0AUse%20dev.&folder=%2Ftmp%2Fshea%20worktree"
        );
        assert_eq!(
            claude_new_session_link("Review #407", None),
            "claude://code/new?q=Review%20%23407"
        );
    }

    #[test]
    fn builds_codex_new_thread_link_with_prompt_and_path() {
        let path = PathBuf::from("/tmp/shea worktree");
        assert_eq!(
            codex_new_thread_link("Review #407\nUse dev.", Some(&path)),
            "codex://threads/new?prompt=Review%20%23407%0AUse%20dev.&path=%2Ftmp%2Fshea%20worktree"
        );
    }
}
