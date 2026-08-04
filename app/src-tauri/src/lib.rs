//! Entry point + composition root (design D2): builds adapters, hands them to the
//! core, registers the protocol. No business logic lives here.

pub mod adapters;
pub mod core;

use adapters::serving::ServeState;
use tauri::Manager;

/// Boot ready-state signal (spec baseline-boot; updater task 6.3 consumes this).
#[tauri::command]
fn shell_ready() {
    println!("SHELL ready");
}

#[tauri::command]
fn shell_failed(error: String) {
    eprintln!("SHELL boot failed: {error}");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Resource root: env override for plain `cargo run` dev sessions (the
            // Tauri CLI copies resources only on `tauri dev`/`tauri build`).
            let resource_root = match std::env::var("STEWARD_RESOURCE_DIR") {
                Ok(dir) => dir.into(),
                Err(_) => app.path().resource_dir()?,
            };
            let store_root = app.path().app_data_dir()?.join("packs");
            let state = ServeState::new(resource_root, store_root)
                .map_err(|e| format!("pack origin init: {e}"))?;
            app.manage(state);
            Ok(())
        })
        .register_uri_scheme_protocol("pack", |ctx, request| {
            let state = ctx.app_handle().state::<ServeState>();
            let path = request.uri().path();
            let (status, mime, body) = state.serve(path);
            println!("PACK {status} {path} ({} bytes)", body.len());
            let mut builder = tauri::http::Response::builder()
                .status(status)
                .header("content-type", &mime);
            if mime == "text/html" {
                // Spike finding (design D1): conf `csp` does not reach custom-protocol
                // responses; the adapter delivers the policy itself.
                builder = builder.header("content-security-policy", state.csp());
            }
            builder
                .body(body)
                .expect("static response parts are always valid")
        })
        .invoke_handler(tauri::generate_handler![shell_ready, shell_failed])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
