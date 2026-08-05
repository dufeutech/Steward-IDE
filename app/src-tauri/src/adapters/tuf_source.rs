//! TUF-backed `UpdateSource` (ADR: Adopt `tough`; design D4).
//!
//! `tough` owns all metadata verification — root chain, timestamp/snapshot/targets
//! order, expiry, version monotonicity — which is exactly the pack-update spec's
//! replay/rollback/freeze/mix-and-match defense. This adapter only maps our port onto
//! a verified repository.
//!
//! Repository layout (static files on GitHub Pages — ADR: Rent):
//! ```text
//! metadata/  root.json, timestamp.json, snapshot.json, targets.json
//! targets/   <pack>.manifest.json          the release description
//!            <hash>                        each blob, addressed by content
//! ```
//!
//! Target names are flat because the publisher's signing tool (`tuftool`, the
//! CLI sibling of this crate) names every target after a file's basename, so no
//! directory structure survives into the namespace. Blob names are content
//! hashes and therefore already globally unique; the pack prefix keeps manifests
//! distinct.

use tough::{IntoVec, Repository, RepositoryLoader, TargetName};

use super::updater::UpdateSource;

pub struct TufSource {
    repo: Repository,
    pack: String,
}

impl TufSource {
    /// Load and verify the remote repository. `root_bytes` is the embedded trust
    /// anchor; `datastore` persists trusted metadata between runs (this is what makes
    /// rollback/freeze detection stick across sessions).
    pub async fn load(
        root_bytes: &[u8],
        metadata_url: &str,
        targets_url: &str,
        datastore: &std::path::Path,
        pack: &str,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(datastore).map_err(|e| e.to_string())?;
        let root = root_bytes.to_vec();
        let repo = RepositoryLoader::new(
            &root,
            metadata_url
                .parse()
                .map_err(|e| format!("metadata url: {e}"))?,
            targets_url
                .parse()
                .map_err(|e| format!("targets url: {e}"))?,
        )
        .datastore(datastore)
        .load()
        .await
        .map_err(|e| format!("TUF load/verify: {e}"))?;
        Ok(Self {
            repo,
            pack: pack.to_string(),
        })
    }

    async fn read(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        let target = TargetName::new(name).map_err(|e| format!("target name {name}: {e}"))?;
        match self.repo.read_target(&target).await {
            Ok(Some(stream)) => Ok(Some(
                stream
                    .into_vec()
                    .await
                    .map_err(|e| format!("read {name}: {e}"))?,
            )),
            Ok(None) => Ok(None),
            Err(e) => Err(format!("read {name}: {e}")),
        }
    }
}

impl UpdateSource for TufSource {
    async fn manifest_bytes(&self) -> Result<Option<Vec<u8>>, String> {
        self.read(&format!("{}.manifest.json", self.pack)).await
    }

    async fn fetch_blob(&self, sha256: &str) -> Result<Vec<u8>, String> {
        self.read(sha256)
            .await?
            .ok_or_else(|| format!("blob {sha256} not in repository"))
    }
}
