//! Request-path → manifest-entry resolution (spec `asset-serving`).
//!
//! Content-agnostic by construction: this module knows paths and hashes, never asset
//! kinds. Media types are the serving adapter's concern, driven by a data file.

use super::manifest::{FileEntry, Manifest};

#[derive(Debug, PartialEq, Eq)]
pub struct PathRejected;

/// Normalize a URL-style relative path or reject it.
///
/// Rejects absolute paths, `.`/`..` segments, backslashes, drive colons, and empty
/// input; collapses duplicate slashes. Every request path passes through here before
/// touching a manifest or the filesystem (spec: path traversal scenario).
pub fn normalize_rel_path(raw: &str) -> Result<String, PathRejected> {
    if raw.contains('\\') || raw.contains(':') {
        return Err(PathRejected);
    }
    let mut segments = Vec::new();
    for seg in raw.split('/') {
        match seg {
            "" | "." => continue,
            ".." => return Err(PathRejected),
            s => segments.push(s),
        }
    }
    if segments.is_empty() {
        return Err(PathRejected);
    }
    Ok(segments.join("/"))
}

/// Look up a normalized relative path in a manifest. `None` is a 404: the file is not
/// part of the pack (spec: a file absent from the manifest is not part of the pack).
pub fn resolve<'m>(manifest: &'m Manifest, raw_path: &str) -> Option<&'m FileEntry> {
    let normalized = normalize_rel_path(raw_path).ok()?;
    manifest.files.iter().find(|f| f.path == normalized)
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
                {"path": "dist/a.js", "size": 3, "sha256": "a".repeat(64)},
                {"path": "dist/editor/w.js", "size": 5, "sha256": "c".repeat(64)}
            ],
            "entry": {"scripts": ["dist/a.js"], "styles": []}
        }))
        .unwrap()
    }

    #[test]
    fn scenario_relative_sibling_resolution() {
        let m = manifest();
        assert_eq!(resolve(&m, "dist/a.js").unwrap().sha256, "a".repeat(64));
        assert_eq!(
            resolve(&m, "dist/editor/w.js").unwrap().path,
            "dist/editor/w.js"
        );
    }

    #[test]
    fn scenario_path_traversal_refused() {
        let m = manifest();
        for attempt in [
            "../secrets.txt",
            "dist/../../x",
            "dist\\a.js",
            "c:/windows/system32",
            "..",
            "",
        ] {
            assert!(resolve(&m, attempt).is_none(), "must refuse {attempt:?}");
        }
    }

    #[test]
    fn normalization_collapses_redundant_segments_only() {
        assert_eq!(normalize_rel_path("./dist//a.js").unwrap(), "dist/a.js");
        assert_eq!(normalize_rel_path("/dist/a.js").unwrap(), "dist/a.js");
        assert_eq!(normalize_rel_path("a/./b").unwrap(), "a/b");
        assert_eq!(normalize_rel_path("a/../b"), Err(PathRejected));
    }

    #[test]
    fn unlisted_file_is_not_part_of_the_pack() {
        assert!(resolve(&manifest(), "dist/ghost.js").is_none());
    }
}
