use std::{fs, path::Path};

use serde::Deserialize;
use tauri::State;

use crate::workspace::{WorkspaceManager, WorkspaceProfile};

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HandoffPromptKind {
    NeedToClarify,
    NeedHumanInput,
    HumanReview,
}

impl HandoffPromptKind {
    fn relative_path(self) -> &'static Path {
        Path::new(match self {
            Self::NeedToClarify => ".shea/prompts/need-to-clarify-handoff.md",
            Self::NeedHumanInput => ".shea/prompts/need-human-input-handoff.md",
            Self::HumanReview => ".shea/prompts/human-review-handoff.md",
        })
    }

    fn label(self) -> &'static str {
        match self {
            Self::NeedToClarify => "Need to Clarify",
            Self::NeedHumanInput => "Need Human Input",
            Self::HumanReview => "Human Review",
        }
    }
}

#[tauri::command]
pub fn get_handoff_prompt(
    kind: HandoffPromptKind,
    manager: State<'_, WorkspaceManager>,
) -> Result<String, String> {
    read_handoff_prompt(&manager.current(), kind)
}

fn read_handoff_prompt(
    workspace: &WorkspaceProfile,
    kind: HandoffPromptKind,
) -> Result<String, String> {
    let target_root = fs::canonicalize(workspace.target_path()).map_err(|error| {
        format!(
            "Unable to resolve the active target workspace {}: {error}",
            workspace.target_root
        )
    })?;
    let relative_path = kind.relative_path();
    let requested_path = target_root.join(relative_path);
    let prompt_path = fs::canonicalize(&requested_path).map_err(|error| {
        format!(
            "{} handoff prompt is missing or unreadable at {}: {error}",
            kind.label(),
            relative_path.display()
        )
    })?;

    if !prompt_path.starts_with(&target_root) {
        return Err(format!(
            "{} handoff prompt resolves outside the active target workspace: {}",
            kind.label(),
            relative_path.display()
        ));
    }
    if !prompt_path.is_file() {
        return Err(format!(
            "{} handoff prompt is not a file: {}",
            kind.label(),
            relative_path.display()
        ));
    }

    let bytes = fs::read(&prompt_path).map_err(|error| {
        format!(
            "Unable to read {} handoff prompt at {}: {error}",
            kind.label(),
            relative_path.display()
        )
    })?;
    let prompt = String::from_utf8(bytes).map_err(|error| {
        format!(
            "{} handoff prompt is not valid UTF-8 at {}: {error}",
            kind.label(),
            relative_path.display()
        )
    })?;
    if prompt.trim().is_empty() {
        return Err(format!(
            "{} handoff prompt is empty at {}",
            kind.label(),
            relative_path.display()
        ));
    }
    Ok(prompt)
}

#[cfg(test)]
mod tests {
    use super::{read_handoff_prompt, HandoffPromptKind};
    use crate::workspace::{WorkspaceManager, WorkspaceProfile};
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "shea-handoff-prompts-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn profile(root: &Path) -> WorkspaceProfile {
        WorkspaceProfile {
            engine_root: root.display().to_string(),
            target_root: root.display().to_string(),
            workflow_path: "workflows/shea-symphony.md".into(),
            cli_path: None,
            source: "test".into(),
            error: None,
        }
    }

    fn write_prompt(root: &Path, kind: HandoffPromptKind, contents: impl AsRef<[u8]>) {
        let path = root.join(kind.relative_path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn maps_only_the_three_allowlisted_prompt_files() {
        assert_eq!(
            HandoffPromptKind::NeedToClarify.relative_path(),
            Path::new(".shea/prompts/need-to-clarify-handoff.md")
        );
        assert_eq!(
            HandoffPromptKind::NeedHumanInput.relative_path(),
            Path::new(".shea/prompts/need-human-input-handoff.md")
        );
        assert_eq!(
            HandoffPromptKind::HumanReview.relative_path(),
            Path::new(".shea/prompts/human-review-handoff.md")
        );
        assert!(serde_json::from_str::<HandoffPromptKind>(r#""../../etc/passwd""#).is_err());
    }

    #[test]
    fn reads_runtime_sentinel_instead_of_a_bundled_prompt() {
        let target = TestDir::new();
        let sentinel = format!("runtime sentinel {}", std::process::id());
        write_prompt(target.path(), HandoffPromptKind::HumanReview, &sentinel);

        assert_eq!(
            read_handoff_prompt(&profile(target.path()), HandoffPromptKind::HumanReview).unwrap(),
            sentinel
        );
    }

    #[test]
    fn follows_the_current_workspace_after_target_switches() {
        let engine = TestDir::new();
        let target_a = TestDir::new();
        let target_b = TestDir::new();
        let store = TestDir::new();
        write_prompt(
            target_a.path(),
            HandoffPromptKind::NeedHumanInput,
            "target A",
        );
        write_prompt(
            target_b.path(),
            HandoffPromptKind::NeedHumanInput,
            "target B",
        );
        let manager = WorkspaceManager::new(
            engine.path().to_path_buf(),
            profile(target_a.path()),
            store.path().join("profile.json"),
        );

        assert_eq!(
            read_handoff_prompt(&manager.current(), HandoffPromptKind::NeedHumanInput).unwrap(),
            "target A"
        );
        manager
            .set_target(Some(target_b.path().display().to_string()))
            .unwrap();
        assert_eq!(
            read_handoff_prompt(&manager.current(), HandoffPromptKind::NeedHumanInput).unwrap(),
            "target B"
        );
    }

    #[test]
    fn reloads_prompt_contents_after_the_file_changes() {
        let target = TestDir::new();
        write_prompt(target.path(), HandoffPromptKind::NeedToClarify, "first");
        assert_eq!(
            read_handoff_prompt(&profile(target.path()), HandoffPromptKind::NeedToClarify).unwrap(),
            "first"
        );

        write_prompt(target.path(), HandoffPromptKind::NeedToClarify, "second");
        assert_eq!(
            read_handoff_prompt(&profile(target.path()), HandoffPromptKind::NeedToClarify).unwrap(),
            "second"
        );
    }

    #[test]
    fn rejects_missing_empty_and_non_utf8_prompts() {
        let target = TestDir::new();
        let workspace = profile(target.path());
        let missing = read_handoff_prompt(&workspace, HandoffPromptKind::HumanReview).unwrap_err();
        assert!(missing.contains("missing or unreadable"));

        write_prompt(target.path(), HandoffPromptKind::HumanReview, "  \n\t");
        let empty = read_handoff_prompt(&workspace, HandoffPromptKind::HumanReview).unwrap_err();
        assert!(empty.contains("is empty"));

        write_prompt(target.path(), HandoffPromptKind::HumanReview, [0xff, 0xfe]);
        let non_utf8 = read_handoff_prompt(&workspace, HandoffPromptKind::HumanReview).unwrap_err();
        assert!(non_utf8.contains("not valid UTF-8"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_unreadable_prompts() {
        use std::os::unix::fs::PermissionsExt;

        let target = TestDir::new();
        write_prompt(target.path(), HandoffPromptKind::HumanReview, "secret");
        let path = target
            .path()
            .join(HandoffPromptKind::HumanReview.relative_path());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let result = read_handoff_prompt(&profile(target.path()), HandoffPromptKind::HumanReview);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        assert!(result.unwrap_err().contains("Unable to read"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_that_escape_the_target_root() {
        use std::os::unix::fs::symlink;

        let target = TestDir::new();
        let outside = TestDir::new();
        let outside_prompt = outside.path().join("outside.md");
        fs::write(&outside_prompt, "outside").unwrap();
        let prompt_path = target
            .path()
            .join(HandoffPromptKind::HumanReview.relative_path());
        fs::create_dir_all(prompt_path.parent().unwrap()).unwrap();
        symlink(outside_prompt, prompt_path).unwrap();

        let error = read_handoff_prompt(&profile(target.path()), HandoffPromptKind::HumanReview)
            .unwrap_err();
        assert!(error.contains("outside the active target workspace"));
    }
}
