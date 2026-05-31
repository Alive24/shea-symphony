mod autoloop;
mod autoloop_events;
mod autoloop_state;
mod cli;
mod github;
mod read_surfaces;

use autoloop_state::LoopManager;

fn main() {
    tauri::Builder::default()
        .manage(LoopManager::default())
        .invoke_handler(tauri::generate_handler![
            autoloop::start_autoloop,
            autoloop::stop_autoloop,
            autoloop::get_loop_state,
            read_surfaces::get_runtime_snapshot,
            github::get_github_user,
            read_surfaces::get_operator_overview,
            read_surfaces::get_read_surface
        ])
        .run(tauri::generate_context!())
        .expect("error while running Shea Symphony App");
}
