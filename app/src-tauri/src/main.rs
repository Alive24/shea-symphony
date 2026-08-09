mod autoloop;
mod autoloop_events;
mod autoloop_state;
mod cli;
mod external_links;
mod github;
mod handoff_prompts;
mod read_surfaces;
mod runtime;
mod target_context;
mod target_runtime;
mod temporal_health;
mod workspace;

use autoloop_state::LoopManager;
use workspace::{default_profile_path, initial_workspace_profile, WorkspaceManager};

fn main() {
    runtime::publish_installed_discovery_if_available()
        .expect("failed to prepare the bundled Shea Symphony Legacy runtime");
    let engine_root = cli::repo_root();
    let profile_path = default_profile_path();
    let args = std::env::args_os().collect::<Vec<_>>();
    let workspace_profile = initial_workspace_profile(engine_root.clone(), &args, &profile_path);

    tauri::Builder::default()
        .manage(LoopManager::default())
        .manage(WorkspaceManager::new(
            engine_root,
            workspace_profile,
            profile_path,
        ))
        .invoke_handler(tauri::generate_handler![
            workspace::get_workspace_profile,
            workspace::set_active_workspace,
            autoloop::start_autoloop,
            autoloop::stop_autoloop,
            autoloop::get_loop_state,
            read_surfaces::get_runtime_snapshot,
            github::get_github_user,
            read_surfaces::get_operator_overview,
            read_surfaces::get_read_surface,
            read_surfaces::get_codex_transcript,
            github::get_issue_timeline,
            handoff_prompts::get_handoff_prompt,
            external_links::open_codex_thread,
            external_links::open_github_source,
            external_links::open_handoff_target,
            external_links::open_codex_handoff,
            target_runtime::get_target_runtime_state,
            target_runtime::initialize_target_runtime_state,
            temporal_health::get_temporal_runtime_health
        ])
        .run(tauri::generate_context!())
        .expect("error while running Shea Symphony App");
}
