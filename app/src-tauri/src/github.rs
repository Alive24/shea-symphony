use std::process::Command;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubUserSnapshot {
    available: bool,
    login: String,
    name: String,
    email: String,
    avatar_url: String,
    error: String,
}

#[tauri::command]
pub async fn get_github_user() -> Result<GitHubUserSnapshot, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let output = Command::new("gh")
            .args(["api", "user"])
            .output()
            .map_err(|error| format!("failed to run gh api user: {error}"))?;
        if !output.status.success() {
            return Ok(GitHubUserSnapshot {
                available: false,
                login: String::new(),
                name: String::new(),
                email: String::new(),
                avatar_url: String::new(),
                error: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        let user: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("invalid gh user JSON: {error}"))?;
        Ok(GitHubUserSnapshot {
            available: true,
            login: user
                .get("login")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            name: user
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            email: user
                .get("email")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            avatar_url: user
                .get("avatar_url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            error: String::new(),
        })
    })
    .await
    .map_err(|error| format!("github user task failed: {error}"))?
}
