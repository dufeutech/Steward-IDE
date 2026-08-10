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
/// Terminal context events (change `terminal-surface`, design D8). Output bytes are
/// deliberately absent: they travel per-session over an IPC channel, not on this bus.
const EVENT_SESSION_STARTED: &str = "event:terminal.session_started";
const EVENT_SESSION_EXITED: &str = "event:terminal.session_exited";

/// The surface that may start a session (design D6, layer 3).
const APPLICATION_COMPOSITION: &str = "application";

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

// ---------------------------------------------------------------------------------------
// Terminal context (change `terminal-surface`). The commands below are thin: they
// translate, delegate to `core::terminal`, and render the result. Every rule they appear
// to enforce is enforced in the core.
// ---------------------------------------------------------------------------------------

/// Live sessions. Managed state beside `ServeState`, never handed to the webview, which
/// only ever holds an opaque identifier (design D5).
struct Sessions(std::sync::Mutex<core::terminal::Registry>);

/// The spawner the sessions are opened on. Held as the port, not the implementation, so
/// nothing here depends on which PTY library is in use.
struct Spawner(Box<dyn core::terminal::PtySpawner>);

/// Errors cross the IPC boundary as their `Display` text: the surface has to be able to
/// state a reason, and the variants are a Rust concern.
fn reason(e: core::terminal::SessionError) -> String {
    e.to_string()
}

/// Start a session (spec `terminal-session`).
///
/// The composition gate is here rather than in the core because "which surface is being
/// served" is a fact about the assets context, not about terminals.
#[tauri::command]
fn terminal_open(
    app: AppHandle,
    serve: State<ServeState>,
    sessions: State<Sessions>,
    spawner: State<Spawner>,
    columns: i64,
    rows: i64,
    on_output: tauri::ipc::Channel,
) -> Result<u64, String> {
    if serve.composition_marker() != APPLICATION_COMPOSITION {
        // The bootstrap recovery surface must never be able to start a shell. A Tauri
        // capability cannot express this: it is scoped per window, and both surfaces
        // render in `main` (design D6).
        return Err("no session: the application surface is not being served".into());
    }
    let config = serve
        .terminal()
        .ok_or("no session: this build declares no terminal configuration")?;

    let size = adapters::terminal_ipc::requested_size(columns, rows).map_err(reason)?;
    let program = adapters::terminal_ipc::shell_for_this_platform(config).map_err(reason)?;
    let sink = adapters::terminal_ipc::output_sink(on_output);

    let exit_app = app.clone();
    let id = sessions
        .0
        .lock()
        .expect("poisoned")
        .open(
            spawner.0.as_ref(),
            size,
            program.clone(),
            sink,
            move |id, cause| {
                // The shell stopped. Mark it so later writes report "ended" rather than
                // "unknown", then publish the fact.
                if let Some(state) = exit_app.try_state::<Sessions>() {
                    state
                        .0
                        .lock()
                        .expect("poisoned")
                        .mark_ended(id, cause.clone());
                }
                let _ = exit_app.emit(
                    EVENT_SESSION_EXITED,
                    serde_json::json!({
                        "session_id": id.get(),
                        "cause": cause.tag(),
                        "code": cause.code(),
                        "detail": cause.detail(),
                    }),
                );
            },
        )
        .map_err(reason)?;

    println!("terminal: session {id} started on {program}");
    let _ = app.emit(
        EVENT_SESSION_STARTED,
        serde_json::json!({
            "session_id": id.get(),
            "columns": size.columns,
            "rows": size.rows,
        }),
    );
    Ok(id.get())
}

/// Send input to a session.
///
/// Takes the whole request because the body *is* the payload: raw bytes, never JSON, so a
/// keystroke or a paste reaches the shell exactly as typed (ADR "Byte transport").
#[tauri::command]
fn terminal_write(
    sessions: State<Sessions>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), String> {
    let id = adapters::terminal_ipc::addressed_session(&request).map_err(reason)?;
    let bytes = adapters::terminal_ipc::raw_body(&request).map_err(reason)?;
    sessions
        .0
        .lock()
        .expect("poisoned")
        .write(id, bytes)
        .map_err(reason)
}

/// Tell a session how much room it has (spec `terminal-session`).
#[tauri::command]
fn terminal_resize(
    sessions: State<Sessions>,
    session: u64,
    columns: i64,
    rows: i64,
) -> Result<(), String> {
    let size = adapters::terminal_ipc::requested_size(columns, rows).map_err(reason)?;
    sessions
        .0
        .lock()
        .expect("poisoned")
        .resize(core::terminal::SessionId::new(session), size)
        .map_err(reason)
}

/// End a session and release what it holds.
#[tauri::command]
fn terminal_close(sessions: State<Sessions>, session: u64) -> Result<(), String> {
    sessions
        .0
        .lock()
        .expect("poisoned")
        .close(core::terminal::SessionId::new(session))
        .map_err(reason)
}

/// What the surface needs at startup — a query: it returns data and changes nothing
/// (Rule 10). Deliberately not the whole config.
#[tauri::command]
fn terminal_config(serve: State<ServeState>) -> Result<core::terminal::SurfaceConfig, String> {
    serve
        .terminal()
        .map(core::terminal::SurfaceConfig::from)
        .ok_or_else(|| "this build declares no terminal configuration".into())
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

            // Sessions start where a terminal the user opened themselves would: their home
            // directory. Resolved once, here, so the adapter is handed a decided value
            // rather than discovering one (spec `terminal-session`).
            let session_cwd = app
                .path()
                .home_dir()
                .ok()
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| resource_root.clone());
            app.manage(Sessions(std::sync::Mutex::new(
                core::terminal::Registry::new(),
            )));
            app.manage(Spawner(Box::new(adapters::pty::NativePtySpawner::new(
                session_cwd,
            ))));

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
            retry_acquisition,
            terminal_open,
            terminal_write,
            terminal_resize,
            terminal_close,
            terminal_config
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Nothing a session started may survive the application (spec
            // `terminal-session`: "Sessions do not outlive the application"). Closing at
            // `Exit` covers every route out — window close, quit, or a signal Tauri
            // surfaces — which a per-window handler would not.
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(sessions) = app.try_state::<Sessions>() {
                    sessions.0.lock().expect("poisoned").close_all();
                }
            }
        });
}
