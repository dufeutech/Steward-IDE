//! Adapters: everything that touches an actual technology (filesystem, Tauri protocol,
//! HTTP, crypto) lives here, behind the core's vocabulary. Dependencies point inward.

/// Raising a console control event is Windows' answer to a question Unix answers with a
/// byte, so this module has no Unix counterpart to sit beside (design D2/D4).
#[cfg(windows)]
pub mod console_ctrl;
pub mod fs_store;
pub mod pty;
pub mod serving;
pub mod terminal_ipc;
pub mod tuf_source;
pub mod updater;
