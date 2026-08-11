//! Pure core of the terminal context (openspec change `terminal-surface`, design D4).
//!
//! A second bounded context alongside the assets context. It imports nothing from
//! `core::{config, manifest, resolve, shell, updater, verify}` and nothing from
//! `adapters`; the two contexts meet only in the composition root and on the event bus.
//!
//! Nothing here performs I/O or names a concrete PTY library. It decides — validate,
//! issue, refuse, classify — and returns values; adapters carry them out.

pub mod config;
pub mod registry;
pub mod session;

pub use config::{resolve_shell, NoShell, ShellConfig, SurfaceConfig, TerminalConfig};
pub use registry::Registry;
pub use session::{
    ExitCause, Pty, PtySpawner, SessionError, SessionId, Size, SizeRejected, SpawnRequest,
};
