//! Full-version verification (specs `pack-manifest`, `pack-update`, `pack-store`).
//!
//! Pure: the caller supplies what exists (hash → observed size, plus any staged paths);
//! this module answers whether the version is complete and exact. It never reads disk.

use std::collections::{HashMap, HashSet};

use super::manifest::Manifest;

#[derive(Debug, PartialEq, Eq)]
pub enum VersionIssue {
    /// A manifest file's blob is absent from the store.
    MissingBlob { path: String, sha256: String },
    /// A blob exists but its size disagrees with the manifest (corruption signal;
    /// hash re-verification on read is the store adapter's duty).
    SizeMismatch {
        path: String,
        expected: u64,
        found: u64,
    },
    /// A file present in the staged tree that the manifest does not list
    /// (spec: unlisted files cause verification failure).
    UnlistedFile { path: String },
}

/// Verify that `available` (content hash → byte size) plus `staged_paths` (what is
/// physically present for this version, when staging into a plain tree) exactly
/// satisfies the manifest. Empty result = version is complete and activatable.
pub fn verify_version(
    manifest: &Manifest,
    available: &HashMap<String, u64>,
    staged_paths: Option<&HashSet<String>>,
) -> Vec<VersionIssue> {
    let mut issues = Vec::new();
    for f in &manifest.files {
        match available.get(&f.sha256) {
            None => issues.push(VersionIssue::MissingBlob {
                path: f.path.clone(),
                sha256: f.sha256.clone(),
            }),
            Some(&found) if found != f.size => issues.push(VersionIssue::SizeMismatch {
                path: f.path.clone(),
                expected: f.size,
                found,
            }),
            Some(_) => {}
        }
    }
    if let Some(staged) = staged_paths {
        let listed: HashSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
        for path in staged {
            if !listed.contains(path.as_str()) {
                issues.push(VersionIssue::UnlistedFile { path: path.clone() });
            }
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        serde_json::from_value(serde_json::json!({
            "format_version": 1,
            "id": "pack:assets.xkin",
            "version": "0.1.0",
            "files": [
                {"path": "a.js", "size": 3, "sha256": "a".repeat(64)},
                {"path": "b.css", "size": 4, "sha256": "b".repeat(64)}
            ],
            "entry": {"scripts": ["a.js"], "styles": ["b.css"]}
        }))
        .unwrap()
    }

    fn full_store() -> HashMap<String, u64> {
        HashMap::from([("a".repeat(64), 3), ("b".repeat(64), 4)])
    }

    #[test]
    fn complete_version_has_no_issues() {
        assert!(verify_version(&manifest(), &full_store(), None).is_empty());
    }

    #[test]
    fn scenario_missing_blob_makes_version_incomplete() {
        let mut store = full_store();
        store.remove(&"b".repeat(64));
        let issues = verify_version(&manifest(), &store, None);
        assert_eq!(
            issues,
            vec![VersionIssue::MissingBlob {
                path: "b.css".into(),
                sha256: "b".repeat(64)
            }]
        );
    }

    #[test]
    fn scenario_corrupt_blob_detected_via_size() {
        let mut store = full_store();
        store.insert("a".repeat(64), 999);
        let issues = verify_version(&manifest(), &store, None);
        assert_eq!(
            issues,
            vec![VersionIssue::SizeMismatch {
                path: "a.js".into(),
                expected: 3,
                found: 999
            }]
        );
    }

    #[test]
    fn scenario_unlisted_staged_file_fails_verification() {
        let staged: HashSet<String> = ["a.js", "b.css", "sneaky.js"].map(String::from).into();
        let issues = verify_version(&manifest(), &full_store(), Some(&staged));
        assert_eq!(
            issues,
            vec![VersionIssue::UnlistedFile {
                path: "sneaky.js".into()
            }]
        );
    }
}
