//! Entry point + composition root (design D2): builds adapters, hands them to the
//! core, registers the protocol. No business logic lives here.

pub mod adapters;
pub mod core;

use adapters::serving::ServeState;
use adapters::updater::rollback_unconfirmed;
use tauri::{AppHandle, Emitter, Manager, State};

/// Registry event names (Rule 11; ADR D7). Described in
/// `schemas/events.asyncapi.yaml` — keep the two in sync.
const EVENT_PACK_ACTIVATED: &str = "event:assets.pack_activated";
const EVENT_PACK_ROLLED_BACK: &str = "event:assets.pack_rolled_back";
const EVENT_ACQUISITION_PROGRESSED: &str = "event:assets.acquisition_progressed";
const EVENT_ACQUISITION_FAILED: &str = "event:assets.acquisition_failed";

fn emit_pack_event(app: &AppHandle, event: &str, pack: &str, id: &str, version: &str) {
    let _ = app.emit(
        event,
        serde_json::json!({ "pack": pack, "id": id, "version": version }),
    );
}

/// Boot ready-state signal (spec baseline-boot): the activated version booted, so its
/// pending marker clears and it becomes the retained rollback baseline.
#[tauri::command]
fn shell_ready(state: State<ServeState>) {
    println!("SHELL ready");
    for (pack, _) in state.pack_ids() {
        let _ = state.store().take_pending(&pack);
    }
}

/// Boot failure: roll back any unconfirmed pack and reload the webview
/// (spec pack-store: new version fails to boot → previous reactivates).
#[tauri::command]
fn shell_failed(app: AppHandle, state: State<ServeState>, error: String) {
    eprintln!("SHELL boot failed: {error}");
    let mut rolled_any = false;
    for (pack, id) in state.pack_ids() {
        if let Ok(Some(bad)) = state.store().take_pending(&pack) {
            if let Ok(Some(restored)) = state.store().rollback(&pack) {
                eprintln!("pack {pack}: {bad} failed to boot; rolled back to {restored}");
                state.invalidate(&pack);
                emit_pack_event(&app, EVENT_PACK_ROLLED_BACK, &pack, &id, &restored);
                rolled_any = true;
            }
        }
    }
    if rolled_any {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.eval("location.reload()");
        }
    }
}

/// One acquisition pass over every application pack. Startup and the retry command share
/// it, so a retry is the same operation rather than a second implementation of it
/// (spec bootstrap-shell: retry re-attempts acquisition in the same session).
async fn acquire_all(app: AppHandle) {
    let state = app.state::<ServeState>();
    let Some(endpoint) = state.update_endpoint().cloned() else {
        return;
    };
    let root_path = state.tuf_root().to_path_buf();
    let Ok(root_bytes) = std::fs::read(&root_path) else {
        eprintln!("updater: no embedded TUF root at {root_path:?}; skipping");
        return;
    };
    let Ok(datastore) = app.path().app_data_dir().map(|d| d.join("tuf-datastore")) else {
        return;
    };

    for (pack, id) in state.application_pack_ids() {
        let fail = |kind: &str, reason: String| {
            eprintln!("updater: {pack}: {reason}");
            let _ = app.emit(
                EVENT_ACQUISITION_FAILED,
                serde_json::json!({"pack": &pack, "id": &id, "kind": kind, "reason": reason}),
            );
        };

        let source = match adapters::tuf_source::TufSource::load(
            &root_bytes,
            &endpoint.metadata_url,
            &endpoint.targets_url,
            &datastore.join(&pack),
            &pack,
        )
        .await
        {
            Ok(s) => s,
            // Nothing was reached or what was reached did not verify; either way the
            // client never got usable release metadata.
            Err(e) => {
                fail("transport", e.to_string());
                continue;
            }
        };

        let on_progress = |p: core::updater::Progress| {
            let _ = app.emit(
                EVENT_ACQUISITION_PROGRESSED,
                serde_json::json!({
                    "pack": &pack, "id": &id,
                    "done_bytes": p.done_bytes, "total_bytes": p.total_bytes
                }),
            );
        };

        use adapters::updater::{run_update, UpdateOutcome};
        match run_update(
            state.store(),
            state.manifest_schema(),
            &source,
            &pack,
            &on_progress,
        )
        .await
        {
            UpdateOutcome::Activated { version } => {
                println!("updater: {pack}@{version} activated (pending boot)");
                state.invalidate(&pack);
                emit_pack_event(&app, EVENT_PACK_ACTIVATED, &pack, &id, &version);
            }
            UpdateOutcome::UpToDate => {}
            UpdateOutcome::Failed { kind, reason } => fail(kind.as_str(), reason),
        }
    }
}

/// Retry acquisition without restarting (spec bootstrap-shell). Thin: it starts the same
/// pass startup runs and carries no logic of its own.
#[tauri::command]
fn retry_acquisition(app: AppHandle) {
    tauri::async_runtime::spawn(acquire_all(app));
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
            let state = ServeState::new(resource_root.clone(), store_root)
                .map_err(|e| format!("pack origin init: {e}"))?;

            // A pending marker surviving to this point means the last activation
            // never confirmed — roll back before serving anything.
            let ids: std::collections::HashMap<String, String> =
                state.pack_ids().into_iter().collect();
            for (pack, restored) in rollback_unconfirmed(state.store()) {
                let id = ids.get(&pack).cloned().unwrap_or_default();
                emit_pack_event(app.handle(), EVENT_PACK_ROLLED_BACK, &pack, &id, &restored);
            }

            app.manage(state);

            // Background acquisition (spec pack-update: never blocks startup). With no
            // active version this is also what fills a fresh install, so the bootstrap
            // surface renders first and watches it happen.
            tauri::async_runtime::spawn(acquire_all(app.handle().clone()));
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
        .invoke_handler(tauri::generate_handler![
            shell_ready,
            shell_failed,
            retry_acquisition
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
