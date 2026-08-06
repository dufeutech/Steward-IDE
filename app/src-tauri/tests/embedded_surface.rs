//! Invariants of the embedded recovery surface (spec `bootstrap-shell`, ADR
//! "Embedded-size budget enforcement").
//!
//! These guard the two properties that make the bootstrap surface worth embedding: it is
//! small enough that the binary carries no application weight, and it is self-sufficient
//! enough to render when nothing else works. Both are cheap to lose by accident, so they
//! are asserted rather than intended.

use std::path::{Path, PathBuf};

fn embedded_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("packs-baseline")
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("embedded resource dir is readable") {
            let path = entry.expect("readable dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// What the binary may carry as embedded pack content, in bytes.
///
/// This exists because the failure mode is accretion, not one careless commit: a
/// recovery surface that grows features becomes a second application, and a re-embedded
/// application pack restores the double payment this change removed.
///
/// The number is a tripwire, not a design target. It is deliberately loose enough that a
/// recovery surface with a logo, icons, a few locales, and a real diagnostics view fits
/// without argument — whoever adopts this shape should get to decide what their surface
/// looks like — while still being two orders of magnitude below an application pack, so
/// re-embedding one is caught immediately.
const DEFAULT_EMBEDDED_BUDGET_BYTES: u64 = 256 * 1024;

/// Override for a single run, e.g. while deciding what a bigger surface should cost.
/// Changing the committed default is the deliberate act; this is the experiment.
const BUDGET_ENV: &str = "STEWARD_EMBEDDED_BUDGET_BYTES";

fn embedded_budget_bytes() -> (u64, &'static str) {
    match std::env::var(BUDGET_ENV) {
        Ok(raw) => match raw.trim().parse::<u64>() {
            Ok(bytes) => (bytes, "override"),
            Err(e) => panic!("{BUDGET_ENV}={raw:?} is not a byte count: {e}"),
        },
        Err(_) => (DEFAULT_EMBEDDED_BUDGET_BYTES, "default"),
    }
}

#[test]
fn scenario_embedded_content_stays_within_its_budget() {
    let (budget, source) = embedded_budget_bytes();
    let root = embedded_root();
    let mut total = 0u64;
    let mut largest: Vec<(u64, String)> = Vec::new();
    for path in files_under(&root) {
        let size = path.metadata().expect("file metadata").len();
        total += size;
        largest.push((
            size,
            path.strip_prefix(&root)
                .unwrap_or(&path)
                .display()
                .to_string(),
        ));
    }
    largest.sort_by_key(|(size, _)| std::cmp::Reverse(*size));
    largest.truncate(5);

    assert!(
        total <= budget,
        "embedded pack content is {total} bytes, over the {budget}-byte budget ({source}) \
         by {over}. Largest: {largest:?}.\n\
         If this is an application payload, publish it instead and let the client acquire \
         it once — that is what the budget exists to catch. If the recovery surface itself \
         has legitimately grown, raise DEFAULT_EMBEDDED_BUDGET_BYTES in this file, or set \
         {BUDGET_ENV} to try a number first.",
        over = total - budget
    );
}

/// Anything that would make the surface depend on something outside the binary. A
/// recovery surface that fetches is a recovery surface that fails exactly when needed.
const REMOTE_MARKERS: &[&str] = &[
    "http://",
    "https://",
    "//fonts.",
    "fetch(",
    "XMLHttpRequest",
    "WebSocket",
    "EventSource",
    "importScripts",
    "@import",
];

#[test]
fn scenario_bootstrap_surface_reaches_no_remote_origin() {
    let pack = embedded_root().join("bootstrap");
    let files = files_under(&pack);
    assert!(
        !files.is_empty(),
        "the bootstrap pack must exist at {pack:?}"
    );

    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue; // binary asset: it cannot carry a URL to follow
        };
        for marker in REMOTE_MARKERS {
            assert!(
                !text.contains(marker),
                "{path:?} contains {marker:?}: the bootstrap surface must render entirely \
                 from embedded content, with no request for its own content leaving the machine"
            );
        }
    }
}

/// The surface must not depend on the application pack or its toolchain — it has to build
/// and render in a tree where no application payload exists at all.
#[test]
fn scenario_bootstrap_surface_is_independent_of_the_application_pack() {
    let pack = embedded_root().join("bootstrap");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(pack.join("manifest.json")).expect("manifest"))
            .expect("manifest parses");

    assert_eq!(manifest["id"], "pack:assets.bootstrap");
    assert!(
        manifest.get("purl").is_none(),
        "the bootstrap pack has no external origin: it is built from first-party source \
         in this repository (spec baseline-regen)"
    );

    for entry in manifest["files"].as_array().expect("files array") {
        let rel = entry["path"].as_str().expect("path is a string");
        assert!(
            pack.join(rel).exists(),
            "manifest lists {rel} but it is not present in the embedded payload"
        );
    }
}
