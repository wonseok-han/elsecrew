mod agent;
mod claude_pilot;
mod detect;
mod editor;
mod error;
mod handoff;
mod install;
mod login;
mod orchestration;
mod proctree;
mod project;
mod status;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(login::LoginSession::default())
        .manage(orchestration::PilotSession::default())
        .manage(handoff::HandoffSession::default())
        .invoke_handler(tauri::generate_handler![
            detect::detect_environment,
            install::install_agent,
            login::login_status,
            login::start_login,
            login::submit_login_code,
            login::cancel_login,
            orchestration::prepare_codex_pilot,
            orchestration::codex_pilot_preflight,
            orchestration::run_codex_pilot,
            orchestration::cancel_codex_pilot,
            orchestration::recover_codex_pilot,
            orchestration::discard_codex_pilot,
            handoff::begin_handoff,
            handoff::release_handoff,
            handoff::run_handoff,
            handoff::cancel_handoff,
            handoff::discard_handoff_workspace,
            handoff::handoff_available,
            project::create_first_project,
            project::run_first_chat,
            project::scan_projects,
            project::default_projects_dir,
            editor::detect_editors,
            editor::open_in_editor,
            status::agent_status,
            status::latest_agent_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
