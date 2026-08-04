//! Pack-manifest parsing and validation (spec `pack-manifest`).
//!
//! Order is contractual: envelope (format_version gate) → JSON Schema → semantic
//! checks. A manifest that fails any step is rejected whole; nothing downstream sees a
//! partially interpreted manifest.

use std::collections::HashSet;

use serde::Deserialize;

/// Highest manifest format this build understands (spec: format version gates loading).
pub const FORMAT_VERSION_SUPPORTED: u64 = 1;

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub format_version: u64,
    pub id: String,
    #[serde(default)]
    pub purl: Option<String>,
    pub version: semver::Version,
    pub files: Vec<FileEntry>,
    pub entry: Entry,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    pub scripts: Vec<String>,
    pub styles: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// Not JSON, or no integer `format_version` at the top level.
    Envelope(String),
    /// `format_version` is newer than this build supports: "update the application".
    FormatTooNew { found: u64, supported: u64 },
    /// JSON Schema violation, named per spec ("error naming the violation").
    Schema(String),
    /// Structurally valid but semantically wrong (duplicate path, dangling entry).
    Semantic(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Envelope(m) => write!(f, "manifest envelope: {m}"),
            Self::FormatTooNew { found, supported } => write!(
                f,
                "manifest format {found} exceeds supported {supported}: update the application to use this pack"
            ),
            Self::Schema(m) => write!(f, "manifest schema violation: {m}"),
            Self::Semantic(m) => write!(f, "manifest semantic error: {m}"),
        }
    }
}

/// Parse and fully validate manifest bytes against the (already loaded) JSON Schema.
///
/// The schema arrives as a parsed value — loading it from disk is the adapter's job
/// (data-not-code: the schema lives in `schemas/pack.manifest.schema.json`).
pub fn parse_and_validate(
    bytes: &[u8],
    schema: &serde_json::Value,
) -> Result<Manifest, ManifestError> {
    // 1. Envelope: JSON + format_version gate, before anything else is interpreted.
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| ManifestError::Envelope(format!("not valid JSON: {e}")))?;
    let format_version = value
        .get("format_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| ManifestError::Envelope("missing integer format_version".into()))?;
    if format_version > FORMAT_VERSION_SUPPORTED {
        return Err(ManifestError::FormatTooNew {
            found: format_version,
            supported: FORMAT_VERSION_SUPPORTED,
        });
    }

    // 2. JSON Schema.
    let validator = jsonschema::validator_for(schema)
        .map_err(|e| ManifestError::Schema(format!("schema itself invalid: {e}")))?;
    if let Some(err) = validator.iter_errors(&value).next() {
        return Err(ManifestError::Schema(format!(
            "{} (at {})",
            err, err.instance_path
        )));
    }

    // 3. Typed deserialization (cannot fail semantically after schema pass, but keep
    //    the error informative rather than unwrapping).
    let manifest: Manifest = serde_json::from_value(value)
        .map_err(|e| ManifestError::Schema(format!("deserialize: {e}")))?;

    // 4. Semantic checks the schema language cannot express.
    let mut seen = HashSet::new();
    for f in &manifest.files {
        if !seen.insert(f.path.as_str()) {
            return Err(ManifestError::Semantic(format!(
                "duplicate file path: {}",
                f.path
            )));
        }
    }
    for entry_path in manifest.entry.scripts.iter().chain(&manifest.entry.styles) {
        if !seen.contains(entry_path.as_str()) {
            return Err(ManifestError::Semantic(format!(
                "entry point not in files: {entry_path}"
            )));
        }
    }

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> serde_json::Value {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/schemas/pack.manifest.schema.json"
        ))
        .expect("schema file must exist");
        serde_json::from_slice(&bytes).expect("schema file must be valid JSON")
    }

    fn valid_manifest_json() -> serde_json::Value {
        serde_json::json!({
            "format_version": 1,
            "id": "pack:assets.xkin",
            "version": "0.1.0",
            "files": [
                {"path": "dist/a.js", "size": 3, "sha256": "a".repeat(64)},
                {"path": "dist/b.css", "size": 4, "sha256": "b".repeat(64)}
            ],
            "entry": {"scripts": ["dist/a.js"], "styles": ["dist/b.css"]}
        })
    }

    fn parse(v: &serde_json::Value) -> Result<Manifest, ManifestError> {
        parse_and_validate(v.to_string().as_bytes(), &schema())
    }

    #[test]
    fn accepts_valid_manifest() {
        let m = parse(&valid_manifest_json()).unwrap();
        assert_eq!(m.id, "pack:assets.xkin");
        assert_eq!(m.version.to_string(), "0.1.0");
        assert_eq!(m.files.len(), 2);
    }

    #[test]
    fn scenario_future_format_version_is_refused_with_clear_error() {
        let mut v = valid_manifest_json();
        v["format_version"] = serde_json::json!(FORMAT_VERSION_SUPPORTED + 1);
        let err = parse(&v).unwrap_err();
        assert!(matches!(err, ManifestError::FormatTooNew { .. }));
        assert!(err.to_string().contains("update the application"));
    }

    #[test]
    fn scenario_malformed_manifest_rejected_naming_violation() {
        let mut v = valid_manifest_json();
        v["id"] = serde_json::json!("Pack:Assets.Bad"); // wrong casing
        let err = parse(&v).unwrap_err();
        assert!(matches!(err, ManifestError::Schema(_)));
        assert!(
            err.to_string().contains("id"),
            "error names the field: {err}"
        );
    }

    #[test]
    fn rejects_not_json_as_envelope_error() {
        let err = parse_and_validate(b"not json", &schema()).unwrap_err();
        assert!(matches!(err, ManifestError::Envelope(_)));
    }

    #[test]
    fn rejects_traversal_path_in_files() {
        let mut v = valid_manifest_json();
        v["files"][0]["path"] = serde_json::json!("../escape.js");
        assert!(matches!(parse(&v).unwrap_err(), ManifestError::Schema(_)));
    }

    #[test]
    fn rejects_backslash_path_in_files() {
        let mut v = valid_manifest_json();
        v["files"][0]["path"] = serde_json::json!("dist\\a.js");
        assert!(matches!(parse(&v).unwrap_err(), ManifestError::Schema(_)));
    }

    #[test]
    fn rejects_duplicate_file_paths() {
        let mut v = valid_manifest_json();
        v["files"][1]["path"] = serde_json::json!("dist/a.js");
        assert!(matches!(parse(&v).unwrap_err(), ManifestError::Semantic(_)));
    }

    #[test]
    fn scenario_entry_point_must_be_listed_in_files() {
        let mut v = valid_manifest_json();
        v["entry"]["scripts"] = serde_json::json!(["dist/ghost.js"]);
        let err = parse(&v).unwrap_err();
        assert!(matches!(err, ManifestError::Semantic(_)));
        assert!(err.to_string().contains("ghost"));
    }

    #[test]
    fn rejects_bad_semver() {
        let mut v = valid_manifest_json();
        v["version"] = serde_json::json!("1.0");
        assert!(matches!(parse(&v).unwrap_err(), ManifestError::Schema(_)));
    }

    #[test]
    fn rejects_uppercase_sha256() {
        let mut v = valid_manifest_json();
        v["files"][0]["sha256"] = serde_json::json!("A".repeat(64));
        assert!(matches!(parse(&v).unwrap_err(), ManifestError::Schema(_)));
    }
}
