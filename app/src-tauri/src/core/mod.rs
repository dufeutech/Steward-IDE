//! Pure core of the asset-pack system (openspec change `asset-pack-system`, design D2).
//!
//! Nothing in this module performs I/O or names a concrete technology. It decides —
//! parse, validate, resolve, verify, plan — and returns values; adapters carry them out.

pub mod config;
pub mod manifest;
pub mod resolve;
pub mod shell;
/// The terminal context (change `terminal-surface`). A sibling bounded context, not part
/// of the asset-pack system — it shares the composition root and nothing else.
pub mod terminal;
pub mod updater;
pub mod verify;

pub use config::{PackConfig, PackRole};
pub use manifest::{Manifest, ManifestError, FORMAT_VERSION_SUPPORTED};
pub use resolve::{normalize_rel_path, PathRejected};
pub use verify::{verify_version, VersionIssue};
