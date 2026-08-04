//! Update orchestration (spec `pack-update`; design D4).
//!
//! `UpdateSource` is the port: it yields *already signature-verified* bytes. The TUF
//! adapter (`tuf_source`) implements it with `tough`, which owns metadata verification
//! (freshness, rollback, mix-and-match). This module owns everything after trust:
//! plan → fetch missing → hash-verify into CAS → record ref → full-version verify →
//! activate with a pending marker. Every step is resumable; nothing partial ever
//! becomes activatable.

use std::collections::HashSet;

use crate::adapters::fs_store::FsStore;
use crate::core;

/// A source of verified release content for one pack. Implementations guarantee the
/// bytes they return were signature-verified (TUF) or are local fixtures (tests).
pub trait UpdateSource {
    /// The release's manifest bytes, or None when the source has no release.
    fn manifest_bytes(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<Vec<u8>>, String>> + Send;
    /// Content for one blob, addressed by hash.
    fn fetch_blob(
        &self,
        sha256: &str,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, String>> + Send;
}

#[derive(Debug, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// No release offered, or offered version already active.
    UpToDate,
    /// New version staged and activated (pending boot confirmation).
    Activated { version: String },
    /// Something failed; the active version is untouched (spec: background and
    /// non-blocking — failure leaves the app fully functional).
    Failed { reason: String },
}

/// Run one update cycle for `pack`. Never touches the active version except by the
/// final atomic activation.
pub async fn run_update<S: UpdateSource>(
    store: &FsStore,
    schema: &serde_json::Value,
    source: &S,
    pack: &str,
) -> UpdateOutcome {
    let fail = |reason: String| UpdateOutcome::Failed { reason };

    let manifest_bytes = match source.manifest_bytes().await {
        Ok(Some(b)) => b,
        Ok(None) => return UpdateOutcome::UpToDate,
        Err(e) => return fail(format!("fetch manifest: {e}")),
    };
    let manifest = match core::manifest::parse_and_validate(&manifest_bytes, schema) {
        Ok(m) => m,
        Err(e) => return fail(e.to_string()),
    };
    let version = manifest.version.to_string();
    if store.active_version(pack).ok().flatten().as_deref() == Some(version.as_str()) {
        return UpdateOutcome::UpToDate;
    }

    // Plan against what's already stored; fetch only the gap (resume for free).
    let available = match store.available_blobs() {
        Ok(a) => a,
        Err(e) => return fail(e.to_string()),
    };
    for entry in core::updater::plan_download(&manifest, &available) {
        let bytes = match source.fetch_blob(&entry.sha256).await {
            Ok(b) => b,
            Err(e) => return fail(format!("fetch {}: {e}", entry.path)),
        };
        // put_blob re-verifies the hash on arrival; a tampered blob dies here.
        if let Err(e) = store.put_blob(&entry.sha256, &bytes) {
            return fail(e.to_string());
        }
    }

    // Full-version verification before anything becomes visible.
    let available = match store.available_blobs() {
        Ok(a) => a,
        Err(e) => return fail(e.to_string()),
    };
    let issues = core::verify_version(&manifest, &available, None::<&HashSet<String>>);
    if !issues.is_empty() {
        return fail(format!("incomplete version: {issues:?}"));
    }

    if let Err(e) = store.put_ref(pack, &version, &manifest_bytes) {
        return fail(e.to_string());
    }
    if let Err(e) = store.set_pending(pack, &version) {
        return fail(e.to_string());
    }
    if let Err(e) = store.activate(pack, &version) {
        return fail(e.to_string());
    }
    UpdateOutcome::Activated { version }
}

/// Boot-time check (spec pack-store: new version fails to boot → automatic rollback).
/// A pending marker surviving to the next startup means that version never confirmed;
/// roll it back. Returns the packs rolled back as (pack, restored_version).
pub fn rollback_unconfirmed(store: &FsStore) -> Vec<(String, String)> {
    let mut rolled = Vec::new();
    let Ok(pending) = store.pending_packs() else {
        return rolled;
    };
    for pack in pending {
        let _ = store.take_pending(&pack);
        if let Ok(Some(restored)) = store.rollback(&pack) {
            eprintln!("pack {pack}: previous boot never confirmed; rolled back to {restored}");
            rolled.push((pack, restored));
        }
    }
    rolled
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;

    fn hash(b: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(b);
        format!("{:x}", h.finalize())
    }

    struct FakeSource {
        manifest: Option<Vec<u8>>,
        blobs: HashMap<String, Vec<u8>>,
    }

    impl UpdateSource for FakeSource {
        async fn manifest_bytes(&self) -> Result<Option<Vec<u8>>, String> {
            Ok(self.manifest.clone())
        }
        async fn fetch_blob(&self, sha256: &str) -> Result<Vec<u8>, String> {
            self.blobs
                .get(sha256)
                .cloned()
                .ok_or_else(|| format!("no such blob {sha256}"))
        }
    }

    fn schema() -> serde_json::Value {
        serde_json::from_slice(
            &std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/schemas/pack.manifest.schema.json"
            ))
            .unwrap(),
        )
        .unwrap()
    }

    fn release(version: &str, files: &[(&str, &[u8])]) -> FakeSource {
        let entries: Vec<_> = files
            .iter()
            .map(|(p, b)| serde_json::json!({"path": p, "size": b.len(), "sha256": hash(b)}))
            .collect();
        let manifest = serde_json::json!({
            "format_version": 1,
            "id": "pack:assets.demo",
            "version": version,
            "files": entries,
            "entry": {"scripts": [files[0].0], "styles": []}
        });
        FakeSource {
            manifest: Some(manifest.to_string().into_bytes()),
            blobs: files.iter().map(|(_, b)| (hash(b), b.to_vec())).collect(),
        }
    }

    fn store() -> (tempfile::TempDir, FsStore) {
        let d = tempfile::tempdir().unwrap();
        let s = FsStore::open(d.path().join("packs")).unwrap();
        (d, s)
    }

    #[tokio::test]
    async fn scenario_staged_verified_activated_with_pending_marker() {
        let (_d, s) = store();
        let src = release("0.2.0", &[("a.js", b"aaa"), ("b.js", b"bbb")]);
        let out = run_update(&s, &schema(), &src, "demo").await;
        assert_eq!(
            out,
            UpdateOutcome::Activated {
                version: "0.2.0".into()
            }
        );
        assert_eq!(s.active_version("demo").unwrap().unwrap(), "0.2.0");
        assert_eq!(s.take_pending("demo").unwrap().unwrap(), "0.2.0");
    }

    #[tokio::test]
    async fn scenario_missing_blob_leaves_active_untouched() {
        let (_d, s) = store();
        let mut src = release("0.2.0", &[("a.js", b"aaa"), ("b.js", b"bbb")]);
        src.blobs.remove(&hash(b"bbb")); // endpoint can't serve one file
        let out = run_update(&s, &schema(), &src, "demo").await;
        assert!(matches!(out, UpdateOutcome::Failed { .. }));
        assert_eq!(s.active_version("demo").unwrap(), None);
    }

    #[tokio::test]
    async fn scenario_tampered_blob_rejected() {
        let (_d, s) = store();
        let mut src = release("0.2.0", &[("a.js", b"aaa")]);
        src.blobs.insert(hash(b"aaa"), b"tampered".to_vec());
        let out = run_update(&s, &schema(), &src, "demo").await;
        assert!(matches!(out, UpdateOutcome::Failed { .. }));
        assert_eq!(s.active_version("demo").unwrap(), None);
    }

    #[tokio::test]
    async fn scenario_partial_download_resumes_without_refetch() {
        let (_d, s) = store();
        // First attempt: b.js unavailable → fails, but a.js landed in CAS.
        let mut src = release("0.2.0", &[("a.js", b"aaa"), ("b.js", b"bbb")]);
        let b_hash = hash(b"bbb");
        let b_bytes = src.blobs.remove(&b_hash).unwrap();
        assert!(matches!(
            run_update(&s, &schema(), &src, "demo").await,
            UpdateOutcome::Failed { .. }
        ));
        assert!(
            s.get_blob(&hash(b"aaa")).unwrap().is_some(),
            "kept for resume"
        );

        // Second attempt: only b.js should be fetched — prove it by making a.js
        // unavailable at the source now.
        src.blobs.insert(b_hash, b_bytes);
        src.blobs.remove(&hash(b"aaa"));
        assert_eq!(
            run_update(&s, &schema(), &src, "demo").await,
            UpdateOutcome::Activated {
                version: "0.2.0".into()
            }
        );
    }

    #[tokio::test]
    async fn scenario_same_version_is_up_to_date() {
        let (_d, s) = store();
        let src = release("0.2.0", &[("a.js", b"aaa")]);
        run_update(&s, &schema(), &src, "demo").await;
        assert_eq!(
            run_update(&s, &schema(), &src, "demo").await,
            UpdateOutcome::UpToDate
        );
    }

    #[tokio::test]
    async fn scenario_no_release_offered() {
        let (_d, s) = store();
        let src = FakeSource {
            manifest: None,
            blobs: HashMap::new(),
        };
        assert_eq!(
            run_update(&s, &schema(), &src, "demo").await,
            UpdateOutcome::UpToDate
        );
    }

    #[tokio::test]
    async fn scenario_unconfirmed_boot_rolls_back_automatically() {
        let (_d, s) = store();
        // v1 activated and confirmed; v2 activated but never confirmed.
        let v1 = release("0.1.0", &[("a.js", b"v1")]);
        run_update(&s, &schema(), &v1, "demo").await;
        s.take_pending("demo").unwrap(); // v1 confirmed
        let v2 = release("0.2.0", &[("a.js", b"v2")]);
        run_update(&s, &schema(), &v2, "demo").await;
        // ...app "crashes" before shell_ready; next boot:
        let rolled = rollback_unconfirmed(&s);
        assert_eq!(rolled, vec![("demo".to_string(), "0.1.0".to_string())]);
        assert_eq!(s.active_version("demo").unwrap().unwrap(), "0.1.0");
    }

    #[tokio::test]
    async fn confirmed_boot_does_not_roll_back() {
        let (_d, s) = store();
        let v1 = release("0.1.0", &[("a.js", b"v1")]);
        run_update(&s, &schema(), &v1, "demo").await;
        s.take_pending("demo").unwrap(); // shell_ready arrived
        assert!(rollback_unconfirmed(&s).is_empty());
        assert_eq!(s.active_version("demo").unwrap().unwrap(), "0.1.0");
    }
}
