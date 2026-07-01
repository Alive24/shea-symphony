use std::path::PathBuf;

use shea_symphony::target_runtime::{initialize_target_runtime, inspect_target_runtime};

pub(crate) fn target_runtime_status(path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&inspect_target_runtime(path)?)?
    );
    Ok(())
}

pub(crate) fn target_runtime_init(path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&initialize_target_runtime(path)?)?
    );
    Ok(())
}
