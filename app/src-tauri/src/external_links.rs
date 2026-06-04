use std::process::Command;

#[tauri::command]
pub fn open_codex_thread(deep_link: String) -> Result<(), String> {
    validate_codex_thread_link(&deep_link)?;
    open_external_url(&deep_link)
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

#[cfg(target_os = "macos")]
fn platform_open_command(url: &str) -> Command {
    let mut command = Command::new("open");
    command.arg(url);
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
}
