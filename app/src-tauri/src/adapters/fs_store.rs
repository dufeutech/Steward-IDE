//! Content-addressed pack store on the filesystem (design D3, spec `pack-store`).
//!
//! Layout under one root:
//! ```text
//! cas/sha256/<aa>/<hash>      immutable blobs, fanned out by 2-hex-char prefix
//! refs/<pack>/<semver>.json   manifest copy = the version; GC roots
//! active/<pack>               file whose content is the active semver
//! previous/<pack>             retained rollback semver
//! ```
//! Activation is write-temp + atomic rename of `active/<pack>` — fully switched or not
//! at all. Blobs are hash-verified on write and on read (corruption is reported, never
//! served).

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub struct FsStore {
    root: PathBuf,
}

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    /// Blob content did not match the hash it is keyed by.
    Corrupt { sha256: String },
    /// Attempted to activate a version whose ref file doesn't exist.
    UnknownVersion { pack: String, version: String },
}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "store io: {e}"),
            Self::Corrupt { sha256 } => write!(f, "store blob corrupt: {sha256}"),
            Self::UnknownVersion { pack, version } => {
                write!(f, "unknown version {pack}@{version}")
            }
        }
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

impl FsStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        for dir in ["cas/sha256", "refs", "active", "previous"] {
            fs::create_dir_all(root.join(dir))?;
        }
        Ok(Self { root })
    }

    fn blob_path(&self, sha256: &str) -> PathBuf {
        self.root
            .join("cas/sha256")
            .join(&sha256[..2])
            .join(sha256)
    }

    /// Store a blob, verifying its content hash on arrival. Idempotent.
    pub fn put_blob(&self, sha256: &str, bytes: &[u8]) -> Result<(), StoreError> {
        if hex_sha256(bytes) != sha256 {
            return Err(StoreError::Corrupt {
                sha256: sha256.into(),
            });
        }
        let path = self.blob_path(sha256);
        if path.exists() {
            return Ok(()); // content-addressed: same key ⇒ same bytes
        }
        fs::create_dir_all(path.parent().expect("blob path has parent"))?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Read a blob, re-verifying its hash — corrupt content is an error, never bytes
    /// (spec: corrupted content detected on read).
    pub fn get_blob(&self, sha256: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let path = self.blob_path(sha256);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if hex_sha256(&bytes) != sha256 {
            return Err(StoreError::Corrupt {
                sha256: sha256.into(),
            });
        }
        Ok(Some(bytes))
    }

    /// hash → size for every stored blob (input to `core::verify_version`).
    pub fn available_blobs(&self) -> Result<HashMap<String, u64>, StoreError> {
        let mut out = HashMap::new();
        let cas = self.root.join("cas/sha256");
        for prefix in fs::read_dir(&cas)? {
            for blob in fs::read_dir(prefix?.path())? {
                let blob = blob?;
                if blob.path().extension().is_some() {
                    continue; // leftover .tmp from a crashed write; GC sweeps it
                }
                out.insert(
                    blob.file_name().to_string_lossy().into_owned(),
                    blob.metadata()?.len(),
                );
            }
        }
        Ok(out)
    }

    /// Record a version: its manifest bytes become the ref file (GC root + refcount).
    pub fn put_ref(
        &self,
        pack: &str,
        version: &str,
        manifest_bytes: &[u8],
    ) -> Result<(), StoreError> {
        let dir = self.root.join("refs").join(pack);
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{version}.json"));
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, manifest_bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn get_ref(&self, pack: &str, version: &str) -> Result<Option<Vec<u8>>, StoreError> {
        match fs::read(self.root.join("refs").join(pack).join(format!("{version}.json"))) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn pointer(&self, kind: &str, pack: &str) -> PathBuf {
        self.root.join(kind).join(pack)
    }

    fn read_pointer(&self, kind: &str, pack: &str) -> Result<Option<String>, StoreError> {
        match fs::read_to_string(self.pointer(kind, pack)) {
            Ok(s) => Ok(Some(s.trim().to_string())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn write_pointer(&self, kind: &str, pack: &str, version: &str) -> Result<(), StoreError> {
        let path = self.pointer(kind, pack);
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, version)?;
        fs::rename(&tmp, &path)?; // the atomic switch
        Ok(())
    }

    pub fn active_version(&self, pack: &str) -> Result<Option<String>, StoreError> {
        self.read_pointer("active", pack)
    }

    pub fn previous_version(&self, pack: &str) -> Result<Option<String>, StoreError> {
        self.read_pointer("previous", pack)
    }

    /// Activate a recorded version atomically; the outgoing active version becomes the
    /// retained rollback target (spec: rollback target retained).
    pub fn activate(&self, pack: &str, version: &str) -> Result<(), StoreError> {
        if self.get_ref(pack, version)?.is_none() {
            return Err(StoreError::UnknownVersion {
                pack: pack.into(),
                version: version.into(),
            });
        }
        if let Some(current) = self.active_version(pack)? {
            if current != version {
                self.write_pointer("previous", pack, &current)?;
            }
        }
        self.write_pointer("active", pack, version)
    }

    /// Reactivate the retained previous version (spec: manual rollback, no re-download).
    pub fn rollback(&self, pack: &str) -> Result<Option<String>, StoreError> {
        match self.previous_version(pack)? {
            Some(prev) => {
                self.write_pointer("active", pack, &prev)?;
                Ok(Some(prev))
            }
            None => Ok(None),
        }
    }

    /// Mark-and-sweep GC. Roots: every hash referenced by any ref file (active,
    /// previous, and all recorded versions — deleting refs is a separate, explicit
    /// retention decision, not GC's). Also sweeps orphaned .tmp files.
    /// Returns the number of blobs deleted.
    pub fn gc(&self) -> Result<usize, StoreError> {
        let mut live: std::collections::HashSet<String> = std::collections::HashSet::new();
        let refs = self.root.join("refs");
        for pack_dir in fs::read_dir(&refs)? {
            for ref_file in fs::read_dir(pack_dir?.path())? {
                let bytes = fs::read(ref_file?.path())?;
                // Refs are validated manifests at write time; tolerate unreadable ones
                // here by treating their hashes as unknown (nothing becomes live).
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if let Some(files) = v.get("files").and_then(|f| f.as_array()) {
                        for f in files {
                            if let Some(h) = f.get("sha256").and_then(|h| h.as_str()) {
                                live.insert(h.to_string());
                            }
                        }
                    }
                }
            }
        }
        let mut deleted = 0;
        let cas = self.root.join("cas/sha256");
        for prefix in fs::read_dir(&cas)? {
            for blob in fs::read_dir(prefix?.path())? {
                let blob = blob?;
                let name = blob.file_name().to_string_lossy().into_owned();
                let is_tmp = blob.path().extension().is_some();
                if is_tmp || !live.contains(&name) {
                    fs::remove_file(blob.path())?;
                    deleted += 1;
                }
            }
        }
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_bytes(paths_hashes: &[(&str, &str, u64)]) -> Vec<u8> {
        let files: Vec<_> = paths_hashes
            .iter()
            .map(|(p, h, s)| serde_json::json!({"path": p, "size": s, "sha256": h}))
            .collect();
        serde_json::json!({
            "format_version": 1,
            "id": "pack:assets.xkin",
            "version": "0.1.0",
            "files": files,
            "entry": {"scripts": [], "styles": []}
        })
        .to_string()
        .into_bytes()
    }

    fn store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = FsStore::open(dir.path().join("packs")).unwrap();
        (dir, s)
    }

    #[test]
    fn blob_roundtrip_and_dedup() {
        let (_d, s) = store();
        let bytes = b"hello".to_vec();
        let hash = hex_sha256(&bytes);
        s.put_blob(&hash, &bytes).unwrap();
        s.put_blob(&hash, &bytes).unwrap(); // idempotent
        assert_eq!(s.get_blob(&hash).unwrap().unwrap(), bytes);
        assert_eq!(s.available_blobs().unwrap().len(), 1);
    }

    #[test]
    fn put_blob_rejects_wrong_hash() {
        let (_d, s) = store();
        assert!(matches!(
            s.put_blob(&"0".repeat(64), b"bytes"),
            Err(StoreError::Corrupt { .. })
        ));
    }

    #[test]
    fn scenario_corrupted_content_detected_on_read() {
        let (_d, s) = store();
        let bytes = b"good".to_vec();
        let hash = hex_sha256(&bytes);
        s.put_blob(&hash, &bytes).unwrap();
        fs::write(s.blob_path(&hash), b"tampered").unwrap();
        assert!(matches!(
            s.get_blob(&hash),
            Err(StoreError::Corrupt { .. })
        ));
    }

    #[test]
    fn scenario_activation_is_atomic_pointer_flip_with_rollback_retained() {
        let (_d, s) = store();
        s.put_ref("xkin", "0.1.0", &manifest_bytes(&[])).unwrap();
        s.put_ref("xkin", "0.2.0", &manifest_bytes(&[])).unwrap();

        s.activate("xkin", "0.1.0").unwrap();
        assert_eq!(s.active_version("xkin").unwrap().unwrap(), "0.1.0");
        assert_eq!(s.previous_version("xkin").unwrap(), None);

        s.activate("xkin", "0.2.0").unwrap();
        assert_eq!(s.active_version("xkin").unwrap().unwrap(), "0.2.0");
        assert_eq!(s.previous_version("xkin").unwrap().unwrap(), "0.1.0");
    }

    #[test]
    fn scenario_manual_rollback_without_redownload() {
        let (_d, s) = store();
        s.put_ref("xkin", "0.1.0", &manifest_bytes(&[])).unwrap();
        s.put_ref("xkin", "0.2.0", &manifest_bytes(&[])).unwrap();
        s.activate("xkin", "0.1.0").unwrap();
        s.activate("xkin", "0.2.0").unwrap();

        assert_eq!(s.rollback("xkin").unwrap().unwrap(), "0.1.0");
        assert_eq!(s.active_version("xkin").unwrap().unwrap(), "0.1.0");
    }

    #[test]
    fn rollback_without_previous_is_none() {
        let (_d, s) = store();
        assert_eq!(s.rollback("xkin").unwrap(), None);
    }

    #[test]
    fn activate_unknown_version_refused() {
        let (_d, s) = store();
        assert!(matches!(
            s.activate("xkin", "9.9.9"),
            Err(StoreError::UnknownVersion { .. })
        ));
    }

    #[test]
    fn scenario_gc_spares_everything_any_ref_references() {
        let (_d, s) = store();
        let keep = b"keep".to_vec();
        let drop_ = b"drop".to_vec();
        let keep_hash = hex_sha256(&keep);
        let drop_hash = hex_sha256(&drop_);
        s.put_blob(&keep_hash, &keep).unwrap();
        s.put_blob(&drop_hash, &drop_).unwrap();
        s.put_ref("xkin", "0.1.0", &manifest_bytes(&[("k.js", &keep_hash, 4)]))
            .unwrap();

        let deleted = s.gc().unwrap();
        assert_eq!(deleted, 1);
        assert!(s.get_blob(&keep_hash).unwrap().is_some());
        assert!(s.get_blob(&drop_hash).unwrap().is_none());
    }

    #[test]
    fn scenario_unchanged_file_shared_across_versions() {
        let (_d, s) = store();
        let shared = b"shared".to_vec();
        let hash = hex_sha256(&shared);
        s.put_blob(&hash, &shared).unwrap();
        s.put_ref("xkin", "0.1.0", &manifest_bytes(&[("a.js", &hash, 6)]))
            .unwrap();
        s.put_ref("xkin", "0.2.0", &manifest_bytes(&[("a.js", &hash, 6)]))
            .unwrap();
        // one stored copy serves both versions; GC keeps it while either ref lives
        assert_eq!(s.available_blobs().unwrap().len(), 1);
        assert_eq!(s.gc().unwrap(), 0);
    }
}
