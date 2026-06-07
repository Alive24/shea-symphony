use std::process::Command;

#[tauri::command]
pub fn open_codex_thread(deep_link: String) -> Result<(), String> {
    validate_codex_thread_link(&deep_link)?;
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
        .strip_prefix("https://github.com/Alive24/shea-symphony/")
        .ok_or_else(|| "Only Shea Symphony GitHub source links can be opened.".to_string())?;
    if is_allowed_github_source_path(path) {
        Ok(())
    } else {
        Err("Only Shea Symphony issue, issue comment, and pull request links can be opened.".into())
    }
}

fn is_allowed_github_source_path(path: &str) -> bool {
    let mut parts = path.split('/');
    match (parts.next(), parts.next()) {
        (Some("issues"), Some(number)) | (Some("pull"), Some(number)) => {
            let number = number.split(['#', '?']).next().unwrap_or_default();
            number.chars().all(|ch| ch.is_ascii_digit()) && !number.is_empty()
        }
        _ => false,
    }
}

fn handoff_target(target_id: &str) -> Result<HandoffTarget, String> {
    match target_id {
        "codex-app" => Ok(HandoffTarget {
            app_name: "Codex",
            display_name: "Codex App",
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
        .map_err(|error| format!("Failed to open Codex link: {error}"))?;
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
    fn rejects_non_shea_github_source_links() {
        assert!(validate_github_source_url("https://example.com").is_err());
        assert!(
            validate_github_source_url("https://github.com/Alive24/shea-symphony/actions").is_err()
        );
        assert!(validate_github_source_url("https://github.com/other/repo/issues/430").is_err());
    }

    #[test]
    fn validates_native_handoff_targets() {
        let codex = handoff_target("codex-app").unwrap();
        assert_eq!(codex.app_name, "Codex");
        assert_eq!(codex.display_name, "Codex App");
        assert!(handoff_target("gemini-cli").is_err());
        assert!(handoff_target("https://example.com").is_err());
    }
}
