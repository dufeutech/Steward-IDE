//! Declaring the terminal commands here is what makes them *permissionable*
//! (change `terminal-surface`, ADR "Execution boundary"; design D6, layer 2).
//!
//! Without this, application-defined commands are reachable from every window and webview
//! the app serves and no capability applies to them. With it, `capabilities/terminal.json`
//! governs who may start a shell.
//!
//! Only the terminal commands are listed. Naming a command here makes it permission-gated,
//! so adding `shell_ready` and friends would silently deny them until a capability granted
//! them back — a change with no benefit and a boot failure attached.

fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "terminal_open",
            "terminal_write",
            "terminal_resize",
            "terminal_interrupt",
            "terminal_close",
            "terminal_config",
        ]),
    ))
    .expect("failed to run tauri-build");
}
