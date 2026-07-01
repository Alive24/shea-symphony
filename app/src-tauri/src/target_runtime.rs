use crate::cli::{parse_json_output, run_shea_read};

#[tauri::command]
pub fn get_target_runtime_state(workspace_path: String) -> Result<serde_json::Value, String> {
    target_runtime_command("status", workspace_path)
}

#[tauri::command]
pub fn initialize_target_runtime_state(workspace_path: String) -> Result<serde_json::Value, String> {
    target_runtime_command("init", workspace_path)
}

fn target_runtime_command(command: &str, workspace_path: String) -> Result<serde_json::Value, String> {
    let run = run_shea_read(&[
        "target-runtime".into(),
        command.into(),
        workspace_path,
    ]);
    if !run.summary.ok {
        return Err(if run.summary.stderr.is_empty() {
            run.summary.stdout_preview
        } else {
            run.summary.stderr
        });
    }
    Ok(parse_json_output(&run.stdout))
}
