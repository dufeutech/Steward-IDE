//! Update planning (spec `pack-update`). Pure: given a validated manifest and what the
//! store already holds, decide what must be fetched. Content-addressing makes this the
//! whole download plan — unchanged files are never fetched (spec `pack-store`).

use std::collections::HashMap;

use super::manifest::{FileEntry, Manifest};

/// Files whose blobs are absent from the store, deduplicated by hash (two manifest
/// entries can share content). Order follows the manifest.
pub fn plan_download<'m>(
    manifest: &'m Manifest,
    available: &HashMap<String, u64>,
) -> Vec<&'m FileEntry> {
    let mut seen = std::collections::HashSet::new();
    manifest
        .files
        .iter()
        .filter(|f| !available.contains_key(&f.sha256) && seen.insert(f.sha256.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        serde_json::from_value(serde_json::json!({
            "format_version": 1,
            "id": "pack:assets.xkin",
            "version": "0.2.0",
            "files": [
                {"path": "a.js", "size": 1, "sha256": "a".repeat(64)},
                {"path": "b.js", "size": 2, "sha256": "b".repeat(64)},
                {"path": "b-copy.js", "size": 2, "sha256": "b".repeat(64)}
            ],
            "entry": {"scripts": ["a.js"], "styles": []}
        }))
        .unwrap()
    }

    #[test]
    fn scenario_unchanged_files_not_downloaded_again() {
        let m = manifest();
        let available = HashMap::from([("a".repeat(64), 1u64)]);
        let plan = plan_download(&m, &available);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].sha256, "b".repeat(64));
    }

    #[test]
    fn duplicate_hashes_planned_once() {
        let m = manifest();
        let plan = plan_download(&m, &HashMap::new());
        assert_eq!(
            plan.len(),
            2,
            "b's content appears once despite two entries"
        );
    }

    #[test]
    fn complete_store_plans_nothing() {
        let m = manifest();
        let available = HashMap::from([("a".repeat(64), 1u64), ("b".repeat(64), 2u64)]);
        assert!(plan_download(&m, &available).is_empty());
    }
}
