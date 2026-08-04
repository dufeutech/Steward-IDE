//! Adapters: everything that touches an actual technology (filesystem, Tauri protocol,
//! HTTP, crypto) lives here, behind the core's vocabulary. Dependencies point inward.

pub mod fs_store;
pub mod serving;
pub mod tuf_source;
pub mod updater;
