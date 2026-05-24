use std::io;
use std::path::Path;
use std::process::Command as ProcessCommand;

pub(crate) fn current_git_branch(workspace_path: &Path) -> Result<Option<String>, io::Error> {
    let output = ProcessCommand::new("git")
        .args(["branch", "--show-current"])
        .current_dir(workspace_path)
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!branch.is_empty()).then_some(branch))
}
