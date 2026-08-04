//! The pack origin: routes `pack://` requests to the shell or to pack content
//! (specs `asset-serving`, `baseline-boot`; design D1/D2/D5).
//!
//! Thin by design: URL parsing and byte shoveling here; all decisions (path
//! normalization, manifest resolution, tag generation) in `core`. The boot fallback
//! chain — active → previous → baseline — lives in `resolve_pack`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::adapters::fs_store::FsStore;
use crate::core::{self, Manifest};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub csp: String,
    pub packs: Vec<PackConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackConfig {
    /// URL segment and store key, e.g. `xkin`.
    pub pack: String,
    /// Registry identifier, e.g. `pack:assets.xkin` (Rule 11, ADR D7).
    pub id: String,
    pub baseline_version: String,
}

/// Where a resolved pack's bytes come from. One serving path, two byte sources —
/// the baseline is "a pack like any other" (spec baseline-boot).
enum Blobs {
    Cas,
    BaselineDir(PathBuf),
}

struct ResolvedPack {
    version: String,
    manifest: Manifest,
    blobs: Blobs,
}

pub struct ServeState {
    config: AppConfig,
    media: HashMap<String, String>,
    schema: serde_json::Value,
    store: FsStore,
    shell_dir: PathBuf,
    baseline_dir: PathBuf,
    resolved: Mutex<HashMap<String, std::sync::Arc<ResolvedPack>>>,
}

impl ServeState {
    /// `resource_root` holds `config/`, `shell/`, `schemas/`, `packs-baseline/`
    /// (bundled as Tauri resources); `store_root` is the writable pack store.
    pub fn new(resource_root: PathBuf, store_root: PathBuf) -> Result<Self, String> {
        let read = |rel: &str| {
            std::fs::read(resource_root.join(rel))
                .map_err(|e| format!("resource {rel}: {e}"))
        };
        let config: AppConfig = serde_json::from_slice(&read("config/app.config.json")?)
            .map_err(|e| format!("app.config.json: {e}"))?;
        let media: HashMap<String, String> =
            serde_json::from_slice::<HashMap<String, serde_json::Value>>(
                &read("config/media_types.json")?,
            )
            .map_err(|e| format!("media_types.json: {e}"))?
            .into_iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
            .collect();
        let schema: serde_json::Value =
            serde_json::from_slice(&read("schemas/pack.manifest.schema.json")?)
                .map_err(|e| format!("manifest schema: {e}"))?;
        let store = FsStore::open(store_root).map_err(|e| e.to_string())?;
        Ok(Self {
            config,
            media,
            schema,
            store,
            shell_dir: resource_root.join("shell"),
            baseline_dir: resource_root.join("packs-baseline"),
            resolved: Mutex::new(HashMap::new()),
        })
    }

    fn media_type(&self, path: &str) -> &str {
        let ext = path.rsplit('.').next().unwrap_or_default();
        self.media
            .get(ext)
            .map(String::as_str)
            .unwrap_or("application/octet-stream")
    }

    /// Boot fallback chain (task 4.4 / spec baseline-boot): active → previous →
    /// baseline. Each candidate must parse and validate; failures log and fall
    /// through. Result is cached for the page lifecycle (activation seam = reload).
    fn resolve_pack(&self, pack: &str) -> Option<std::sync::Arc<ResolvedPack>> {
        if let Some(hit) = self.resolved.lock().expect("poisoned").get(pack) {
            return Some(hit.clone());
        }

        let mut candidates: Vec<(String, Blobs)> = Vec::new();
        if let Ok(Some(v)) = self.store.active_version(pack) {
            candidates.push((v, Blobs::Cas));
        }
        if let Ok(Some(v)) = self.store.previous_version(pack) {
            candidates.push((v, Blobs::Cas));
        }
        if let Some(pc) = self.config.packs.iter().find(|p| p.pack == pack) {
            candidates.push((
                pc.baseline_version.clone(),
                Blobs::BaselineDir(self.baseline_dir.join(pack)),
            ));
        }

        for (version, blobs) in candidates {
            let manifest_bytes = match &blobs {
                Blobs::Cas => match self.store.get_ref(pack, &version) {
                    Ok(Some(b)) => b,
                    _ => {
                        eprintln!("pack {pack}@{version}: ref unreadable, falling back");
                        continue;
                    }
                },
                Blobs::BaselineDir(dir) => match std::fs::read(dir.join("manifest.json")) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("pack {pack} baseline manifest: {e}");
                        continue;
                    }
                },
            };
            match core::manifest::parse_and_validate(&manifest_bytes, &self.schema) {
                Ok(manifest) => {
                    let resolved = std::sync::Arc::new(ResolvedPack {
                        version,
                        manifest,
                        blobs,
                    });
                    self.resolved
                        .lock()
                        .expect("poisoned")
                        .insert(pack.to_string(), resolved.clone());
                    return Some(resolved);
                }
                Err(e) => {
                    eprintln!("pack {pack}@{version}: {e}; falling back");
                    continue;
                }
            }
        }
        None
    }

    fn pack_bytes(&self, rp: &ResolvedPack, rel: &str) -> Option<Vec<u8>> {
        let entry = core::resolve::resolve(&rp.manifest, rel)?;
        match &rp.blobs {
            Blobs::Cas => self.store.get_blob(&entry.sha256).ok().flatten(),
            Blobs::BaselineDir(dir) => {
                // Same integrity guarantee as CAS: bytes must match the manifest hash.
                let bytes = std::fs::read(dir.join(&entry.path)).ok()?;
                let mut h = Sha256::new();
                h.update(&bytes);
                if format!("{:x}", h.finalize()) == entry.sha256 {
                    Some(bytes)
                } else {
                    eprintln!("baseline blob hash mismatch: {}", entry.path);
                    None
                }
            }
        }
    }

    fn shell_index(&self) -> Option<Vec<u8>> {
        let template =
            std::fs::read_to_string(self.shell_dir.join("index.html")).ok()?;
        let mut styles = Vec::new();
        let mut scripts = Vec::new();
        for pc in &self.config.packs {
            let rp = self.resolve_pack(&pc.pack)?;
            let base = format!("/packs/{}/{}", pc.pack, rp.version);
            let (s, j) = core::shell::entry_tags(&rp.manifest, &base);
            styles.push(s);
            scripts.push(j);
        }
        Some(
            core::shell::render_shell(&template, &styles.join("\n    "), &scripts.join("\n    "))
                .into_bytes(),
        )
    }

    /// Route one request path to (status, media type, body). Only active content is
    /// reachable: pack URLs must name the resolved version exactly.
    pub fn serve(&self, path: &str) -> (u16, String, Vec<u8>) {
        let not_found = |what: &str| (404u16, "text/plain".to_string(), what.as_bytes().to_vec());

        if path == "/" || path == "/index.html" {
            return match self.shell_index() {
                Some(body) => (200, "text/html".to_string(), body),
                None => (503, "text/plain".to_string(), b"no pack available".to_vec()),
            };
        }
        if let Some(rest) = path.strip_prefix("/shell/") {
            let Ok(rel) = core::normalize_rel_path(rest) else {
                return (400, "text/plain".into(), b"bad path".to_vec());
            };
            return match std::fs::read(self.shell_dir.join(&rel)) {
                Ok(bytes) => (200, self.media_type(&rel).to_string(), bytes),
                Err(_) => not_found("not found"),
            };
        }
        if let Some(rest) = path.strip_prefix("/packs/") {
            let mut it = rest.splitn(3, '/');
            let (Some(pack), Some(version), Some(rel)) = (it.next(), it.next(), it.next())
            else {
                return not_found("bad pack path");
            };
            let Some(rp) = self.resolve_pack(pack) else {
                return not_found("unknown pack");
            };
            if rp.version != version {
                // Staged/inactive versions are not reachable (spec asset-serving).
                return not_found("not the active version");
            }
            return match self.pack_bytes(&rp, rel) {
                Some(bytes) => (200, self.media_type(rel).to_string(), bytes),
                None => not_found("not found"),
            };
        }
        not_found("not found")
    }

    pub fn csp(&self) -> &str {
        &self.config.csp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a resource root + store with a baseline pack of two files.
    fn fixture() -> (tempfile::TempDir, ServeState) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let js = b"console.log(1);\n".to_vec();
        let css = b"body{}\n".to_vec();
        let hash = |b: &[u8]| {
            let mut h = Sha256::new();
            h.update(b);
            format!("{:x}", h.finalize())
        };

        // resources: config, schema, shell, baseline
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("schemas")).unwrap();
        std::fs::create_dir_all(root.join("shell")).unwrap();
        std::fs::create_dir_all(root.join("packs-baseline/demo/dist")).unwrap();
        std::fs::write(
            root.join("config/app.config.json"),
            serde_json::json!({
                "csp": "default-src 'self'",
                "packs": [{"pack": "demo", "id": "pack:assets.demo", "baseline_version": "0.1.0"}]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            root.join("config/media_types.json"),
            r#"{"js": "text/javascript", "css": "text/css", "html": "text/html"}"#,
        )
        .unwrap();
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/schemas/pack.manifest.schema.json"),
            root.join("schemas/pack.manifest.schema.json"),
        )
        .unwrap();
        std::fs::write(
            root.join("shell/index.html"),
            "<head>%%STYLE_TAGS%%</head><body>%%SCRIPT_TAGS%%</body>",
        )
        .unwrap();
        std::fs::write(root.join("packs-baseline/demo/dist/a.js"), &js).unwrap();
        std::fs::write(root.join("packs-baseline/demo/dist/s.css"), &css).unwrap();
        std::fs::write(
            root.join("packs-baseline/demo/manifest.json"),
            serde_json::json!({
                "format_version": 1,
                "id": "pack:assets.demo",
                "version": "0.1.0",
                "files": [
                    {"path": "dist/a.js", "size": js.len(), "sha256": hash(&js)},
                    {"path": "dist/s.css", "size": css.len(), "sha256": hash(&css)}
                ],
                "entry": {"scripts": ["dist/a.js"], "styles": ["dist/s.css"]}
            })
            .to_string(),
        )
        .unwrap();

        let state = ServeState::new(root.to_path_buf(), root.join("store")).unwrap();
        (dir, state)
    }

    #[test]
    fn scenario_first_launch_serves_baseline_and_generates_tags() {
        let (_d, s) = fixture();
        let (status, mime, body) = s.serve("/");
        assert_eq!(status, 200);
        assert_eq!(mime, "text/html");
        let html = String::from_utf8(body).unwrap();
        assert!(html.contains(r#"src="/packs/demo/0.1.0/dist/a.js""#));
        assert!(html.contains(r#"href="/packs/demo/0.1.0/dist/s.css""#));
    }

    #[test]
    fn scenario_one_resolution_path_for_baseline_blobs() {
        let (_d, s) = fixture();
        let (status, mime, body) = s.serve("/packs/demo/0.1.0/dist/a.js");
        assert_eq!(status, 200);
        assert_eq!(mime, "text/javascript");
        assert_eq!(body, b"console.log(1);\n");
    }

    #[test]
    fn scenario_inactive_version_not_reachable() {
        let (_d, s) = fixture();
        let (status, _, _) = s.serve("/packs/demo/9.9.9/dist/a.js");
        assert_eq!(status, 404);
    }

    #[test]
    fn scenario_traversal_refused() {
        let (_d, s) = fixture();
        for p in [
            "/packs/demo/0.1.0/../../../etc/passwd",
            "/shell/../config/app.config.json",
        ] {
            let (status, _, _) = s.serve(p);
            assert!(status == 400 || status == 404, "{p} must be refused, got {status}");
        }
    }

    #[test]
    fn scenario_store_version_preferred_over_baseline_and_corrupt_store_falls_back() {
        let (_d, s) = fixture();
        // Stage a newer version in the store: same file contents, bumped version.
        let js = b"console.log(2);\n".to_vec();
        let mut h = Sha256::new();
        h.update(&js);
        let jhash = format!("{:x}", h.finalize());
        s.store.put_blob(&jhash, &js).unwrap();
        let manifest = serde_json::json!({
            "format_version": 1,
            "id": "pack:assets.demo",
            "version": "0.2.0",
            "files": [{"path": "dist/a.js", "size": js.len(), "sha256": jhash}],
            "entry": {"scripts": ["dist/a.js"], "styles": []}
        });
        s.store
            .put_ref("demo", "0.2.0", manifest.to_string().as_bytes())
            .unwrap();
        s.store.activate("demo", "0.2.0").unwrap();

        // Downloaded pack takes precedence (spec baseline-boot).
        let (status, _, body) = s.serve("/packs/demo/0.2.0/dist/a.js");
        assert_eq!(status, 200);
        assert_eq!(body, js);

        // Corrupt the active ref → fresh state must fall back to baseline.
        let (dir2, s2) = fixture();
        s2.store.put_ref("demo", "0.3.0", b"{not json").unwrap();
        s2.store
            .put_ref("demo", "0.3.0", b"{\"broken\": true}")
            .unwrap();
        // activate() requires a ref; write pointer via activate on the broken ref
        s2.store.activate("demo", "0.3.0").unwrap();
        let (status, _, body) = s2.serve("/");
        assert_eq!(status, 200, "boots from baseline despite corrupt active ref");
        let html = String::from_utf8(body).unwrap();
        assert!(html.contains("/packs/demo/0.1.0/"), "baseline version serves");
        drop(dir2);
    }
}
