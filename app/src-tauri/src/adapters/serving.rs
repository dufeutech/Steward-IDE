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
use crate::core::{self, Manifest, PackConfig};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub csp: String,
    pub packs: Vec<PackConfig>,
    /// Absent = updater disabled; the app runs on store/baseline content forever
    /// (spec pack-update: failure or absence of the endpoint never blocks use).
    #[serde(default)]
    pub update: Option<UpdateEndpoint>,
    /// Absent = no session can be started. The terminal context reads its settings from
    /// the same document as everything else (one config home), but a config that predates
    /// the terminal must still boot — so this is optional rather than required, and
    /// `terminal_open` refuses with a reason when it is missing.
    #[serde(default)]
    pub terminal: Option<crate::core::terminal::TerminalConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateEndpoint {
    pub metadata_url: String,
    pub targets_url: String,
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
    tuf_root: PathBuf,
    resolved: Mutex<HashMap<String, std::sync::Arc<ResolvedPack>>>,
}

impl ServeState {
    /// `resource_root` holds `config/`, `shell/`, `schemas/`, `packs-baseline/`
    /// (bundled as Tauri resources); `store_root` is the writable pack store.
    pub fn new(resource_root: PathBuf, store_root: PathBuf) -> Result<Self, String> {
        let read = |rel: &str| {
            std::fs::read(resource_root.join(rel)).map_err(|e| format!("resource {rel}: {e}"))
        };
        let config: AppConfig = serde_json::from_slice(&read("config/app.config.json")?)
            .map_err(|e| format!("app.config.json: {e}"))?;
        // A config that cannot boot says so now, not at the first unresolvable pack
        // (design D3).
        core::config::validate_packs(&config.packs).map_err(|e| format!("app.config.json: {e}"))?;
        let media: HashMap<String, String> = serde_json::from_slice::<
            HashMap<String, serde_json::Value>,
        >(&read("config/media_types.json")?)
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
            tuf_root: resource_root.join("tuf/root.json"),
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
        // No embedded_version = the binary embeds no copy of this pack, so it
        // contributes no candidate and resolution simply yields None (spec
        // baseline-boot: that is not a fault).
        if let Some(version) = self
            .config
            .packs
            .iter()
            .find(|p| p.pack == pack)
            .and_then(|pc| pc.embedded_version.clone())
        {
            candidates.push((version, Blobs::BaselineDir(self.baseline_dir.join(pack))));
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
        let template = std::fs::read_to_string(self.shell_dir.join("index.html")).ok()?;
        // The core decides which surface this is; the adapter only resolves and renders
        // (design D2). Resolution is cached, so asking twice costs nothing.
        let composition = core::shell::compose(&self.config.packs, &|pack| {
            self.resolve_pack(pack).is_some()
        });
        let chosen: Vec<&PackConfig> = match &composition {
            core::shell::Composition::Application(packs) => packs.clone(),
            core::shell::Composition::Bootstrap(boot) => {
                for pc in core::config::applications(&self.config.packs) {
                    if self.resolve_pack(&pc.pack).is_none() {
                        // Diagnostics, not a startup failure (spec baseline-boot).
                        eprintln!("pack {}: no version available; serving bootstrap", pc.pack);
                    }
                }
                vec![*boot]
            }
            core::shell::Composition::Nothing => return None,
        };

        let mut styles = Vec::new();
        let mut scripts = Vec::new();
        for pc in chosen {
            let rp = self.resolve_pack(&pc.pack)?;
            let base = format!("/packs/{}/{}", pc.pack, rp.version);
            let (s, j) = core::shell::entry_tags(&rp.manifest, &base);
            styles.push(s);
            scripts.push(j);
        }
        Some(
            core::shell::render_shell(
                &template,
                &styles.join("\n    "),
                &scripts.join("\n    "),
                composition.marker(),
            )
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
                // Not "nothing downloaded yet" — that serves the bootstrap surface. This
                // is the binary's own embedded content being unusable.
                None => (
                    503,
                    "text/plain".to_string(),
                    b"no surface available".to_vec(),
                ),
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
            let (Some(pack), Some(version), Some(rel)) = (it.next(), it.next(), it.next()) else {
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

    pub fn store(&self) -> &FsStore {
        &self.store
    }

    /// Configured pack names (URL segments) with their registry ids.
    pub fn pack_ids(&self) -> Vec<(String, String)> {
        self.config
            .packs
            .iter()
            .map(|p| (p.pack.clone(), p.id.clone()))
            .collect()
    }

    /// Application packs only. The bootstrap pack is embedded and has no published
    /// release, so acquisition must not go looking for one on its behalf.
    pub fn application_pack_ids(&self) -> Vec<(String, String)> {
        core::config::applications(&self.config.packs)
            .map(|p| (p.pack.clone(), p.id.clone()))
            .collect()
    }

    /// The embedded trust anchor (spec pack-update: the root of trust ships with the app).
    pub fn tuf_root(&self) -> &std::path::Path {
        &self.tuf_root
    }

    /// Drop the cached resolution for a pack — the next request re-runs the fallback
    /// chain (the activation seam: pointer flips become visible on reload).
    pub fn invalidate(&self, pack: &str) {
        self.resolved.lock().expect("poisoned").remove(pack);
    }

    pub fn update_endpoint(&self) -> Option<&UpdateEndpoint> {
        self.config.update.as_ref()
    }

    pub fn manifest_schema(&self) -> &serde_json::Value {
        &self.schema
    }

    /// Terminal settings, if this build's config declares any (change `terminal-surface`).
    /// The document has one home and one parse; this hands the terminal context its
    /// section without a second reader of the same file.
    pub fn terminal(&self) -> Option<&crate::core::terminal::TerminalConfig> {
        self.config.terminal.as_ref()
    }

    /// Which surface is currently being served — `"application"`, `"bootstrap"`, or
    /// `"none"`.
    ///
    /// The terminal commands gate on this (design D6, layer 3). Tauri capabilities are
    /// scoped per window and this app renders the recovery surface and the application in
    /// the same `main` window, so a capability grant alone cannot tell them apart.
    pub fn composition_marker(&self) -> &'static str {
        core::shell::compose(&self.config.packs, &|pack| {
            self.resolve_pack(pack).is_some()
        })
        .marker()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a resource root + store with an embedded application pack of two files and
    /// an embedded bootstrap pack of one.
    fn fixture() -> (tempfile::TempDir, ServeState) {
        fixture_opts(true)
    }

    /// `app_embedded = false` is the shipped shape (design D1): the binary embeds only
    /// the bootstrap pack, and the application pack resolves from the store or not at all.
    fn fixture_opts(app_embedded: bool) -> (tempfile::TempDir, ServeState) {
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
        std::fs::create_dir_all(root.join("packs-baseline/boot/dist")).unwrap();
        let mut demo = serde_json::json!({
            "pack": "demo", "id": "pack:assets.demo", "role": "application"
        });
        if app_embedded {
            demo["embedded_version"] = "0.1.0".into();
        }
        std::fs::write(
            root.join("config/app.config.json"),
            serde_json::json!({
                "csp": "default-src 'self'",
                "packs": [
                    demo,
                    {
                        "pack": "boot", "id": "pack:assets.boot",
                        "role": "bootstrap", "embedded_version": "0.0.1"
                    }
                ]
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
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/schemas/pack.manifest.schema.json"
            ),
            root.join("schemas/pack.manifest.schema.json"),
        )
        .unwrap();
        std::fs::write(
            root.join("shell/index.html"),
            r#"<head>%%STYLE_TAGS%%</head><body data-composition="%%COMPOSITION%%">%%SCRIPT_TAGS%%</body>"#,
        )
        .unwrap();
        if app_embedded {
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
        }

        let boot = b"console.log('bootstrap');\n".to_vec();
        std::fs::write(root.join("packs-baseline/boot/dist/boot.js"), &boot).unwrap();
        std::fs::write(
            root.join("packs-baseline/boot/manifest.json"),
            serde_json::json!({
                "format_version": 1,
                "id": "pack:assets.boot",
                "version": "0.0.1",
                "files": [
                    {"path": "dist/boot.js", "size": boot.len(), "sha256": hash(&boot)}
                ],
                "entry": {"scripts": ["dist/boot.js"], "styles": []}
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
            assert!(
                status == 400 || status == 404,
                "{p} must be refused, got {status}"
            );
        }
    }

    #[test]
    fn scenario_no_downloaded_version_and_no_embedded_copy_boots_bootstrap() {
        let (_d, s) = fixture_opts(false);
        // The application pack resolves to nothing, and that is not a fault.
        assert!(s.resolve_pack("demo").is_none());

        let (status, mime, body) = s.serve("/");
        assert_eq!(status, 200, "boot reaches a surface, not a startup failure");
        assert_eq!(mime, "text/html");
        let html = String::from_utf8(body).unwrap();
        assert!(
            html.contains(r#"data-composition="bootstrap""#),
            "the bootstrap surface is composed: {html}"
        );
        assert!(html.contains(r#"src="/packs/boot/0.0.1/dist/boot.js""#));
        assert!(
            !html.contains("/packs/demo/"),
            "no half-composed page: {html}"
        );
    }

    #[test]
    fn scenario_bootstrap_never_composed_when_the_application_is_available() {
        let (_d, s) = fixture_opts(false);
        // Stage and activate a downloaded application version.
        let js = b"console.log(3);\n".to_vec();
        let mut h = Sha256::new();
        h.update(&js);
        let jhash = format!("{:x}", h.finalize());
        s.store.put_blob(&jhash, &js).unwrap();
        let manifest = serde_json::json!({
            "format_version": 1,
            "id": "pack:assets.demo",
            "version": "0.4.0",
            "files": [{"path": "dist/a.js", "size": js.len(), "sha256": jhash}],
            "entry": {"scripts": ["dist/a.js"], "styles": []}
        });
        s.store
            .put_ref("demo", "0.4.0", manifest.to_string().as_bytes())
            .unwrap();
        s.store.activate("demo", "0.4.0").unwrap();
        s.invalidate("demo");

        let (status, _, body) = s.serve("/");
        assert_eq!(status, 200);
        let html = String::from_utf8(body).unwrap();
        assert!(html.contains(r#"data-composition="application""#));
        assert!(html.contains(r#"src="/packs/demo/0.4.0/dist/a.js""#));
        assert!(
            !html.contains("/packs/boot/"),
            "the bootstrap surface is not presented once a version is active: {html}"
        );
    }

    /// A resource root with **two** application packs, only one of which has embedded
    /// content. This is the shape the terminal pack introduces (change `terminal-surface`),
    /// and it is the first time more than one application pack composes the page.
    fn two_application_packs(second_resolves: bool) -> (tempfile::TempDir, ServeState) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let hash = |b: &[u8]| {
            let mut h = Sha256::new();
            h.update(b);
            format!("{:x}", h.finalize())
        };
        for sub in ["config", "schemas", "shell"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(
            root.join("config/app.config.json"),
            serde_json::json!({
                "csp": "default-src 'self'",
                "packs": [
                    {"pack": "editor", "id": "pack:assets.editor",
                     "role": "application", "embedded_version": "0.1.0"},
                    {"pack": "terminal", "id": "pack:assets.terminal",
                     "role": "application", "embedded_version": "0.1.0"},
                    {"pack": "boot", "id": "pack:assets.boot",
                     "role": "bootstrap", "embedded_version": "0.0.1"}
                ]
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
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/schemas/pack.manifest.schema.json"
            ),
            root.join("schemas/pack.manifest.schema.json"),
        )
        .unwrap();
        std::fs::write(
            root.join("shell/index.html"),
            r#"<head>%%STYLE_TAGS%%</head><body data-composition="%%COMPOSITION%%">%%SCRIPT_TAGS%%</body>"#,
        )
        .unwrap();

        // The second pack is skipped entirely when it must not resolve — the shipped
        // shape, where a pack with no local content simply contributes no candidate.
        let packs: &[(&str, &str)] = if second_resolves {
            &[
                ("editor", "pack:assets.editor"),
                ("terminal", "pack:assets.terminal"),
            ]
        } else {
            &[("editor", "pack:assets.editor")]
        };
        for (pack, id) in packs {
            let js = format!("console.log('{pack}');\n").into_bytes();
            std::fs::create_dir_all(root.join(format!("packs-baseline/{pack}/dist"))).unwrap();
            std::fs::write(
                root.join(format!("packs-baseline/{pack}/dist/{pack}.js")),
                &js,
            )
            .unwrap();
            std::fs::write(
                root.join(format!("packs-baseline/{pack}/manifest.json")),
                serde_json::json!({
                    "format_version": 1, "id": id, "version": "0.1.0",
                    "files": [{"path": format!("dist/{pack}.js"),
                               "size": js.len(), "sha256": hash(&js)}],
                    "entry": {"scripts": [format!("dist/{pack}.js")], "styles": []}
                })
                .to_string(),
            )
            .unwrap();
        }
        let boot = b"console.log('bootstrap');\n".to_vec();
        std::fs::create_dir_all(root.join("packs-baseline/boot/dist")).unwrap();
        std::fs::write(root.join("packs-baseline/boot/dist/boot.js"), &boot).unwrap();
        std::fs::write(
            root.join("packs-baseline/boot/manifest.json"),
            serde_json::json!({
                "format_version": 1, "id": "pack:assets.boot", "version": "0.0.1",
                "files": [{"path": "dist/boot.js", "size": boot.len(), "sha256": hash(&boot)}],
                "entry": {"scripts": ["dist/boot.js"], "styles": []}
            })
            .to_string(),
        )
        .unwrap();

        let store = dir.path().join("store");
        let state = ServeState::new(root.to_path_buf(), store).unwrap();
        (dir, state)
    }

    #[test]
    fn scenario_two_application_packs_compose_one_page() {
        let (_d, s) = two_application_packs(true);
        let (status, _, body) = s.serve("/");
        assert_eq!(status, 200);
        let html = String::from_utf8(body).unwrap();
        assert!(html.contains(r#"data-composition="application""#));
        // Both packs' entry points, in configuration order — the terminal surface only
        // loads because the editor pack resolved too, and vice versa.
        let editor = html
            .find("/packs/editor/0.1.0/dist/editor.js")
            .expect("editor");
        let terminal = html
            .find("/packs/terminal/0.1.0/dist/terminal.js")
            .expect("terminal");
        assert!(
            editor < terminal,
            "configuration order is preserved: {html}"
        );
        assert_eq!(
            s.composition_marker(),
            "application",
            "terminal_open's gate agrees with what was served"
        );
    }

    #[test]
    fn scenario_one_of_two_application_packs_is_unresolvable() {
        // The risk the terminal pack introduces: a page missing part of the application is
        // not the application, so an unavailable terminal pack costs the user the editor
        // too. Asserted rather than assumed, because it is a deliberate trade-off and a
        // future change must not soften it by accident.
        let (_d, s) = two_application_packs(false);
        let (status, _, body) = s.serve("/");
        assert_eq!(status, 200);
        let html = String::from_utf8(body).unwrap();
        assert!(
            html.contains(r#"data-composition="bootstrap""#),
            "the recovery surface is served, not a half-composed application: {html}"
        );
        assert!(
            !html.contains("/packs/editor/"),
            "the editor is NOT presented on its own: {html}"
        );
        assert_eq!(
            s.composition_marker(),
            "bootstrap",
            "so terminal_open refuses to start a shell (design D6, layer 3)"
        );
    }

    #[test]
    fn scenario_unusable_embedded_content_is_distinct_from_nothing_downloaded() {
        let (dir, s) = fixture_opts(false);
        // Break the embedded bootstrap pack itself: now nothing at all can be served.
        std::fs::write(
            dir.path().join("packs-baseline/boot/manifest.json"),
            b"{nope",
        )
        .unwrap();
        s.invalidate("boot");
        let (status, _, body) = s.serve("/");
        assert_eq!(status, 503);
        assert_eq!(body, b"no surface available");
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
        assert_eq!(
            status, 200,
            "boots from baseline despite corrupt active ref"
        );
        let html = String::from_utf8(body).unwrap();
        assert!(
            html.contains("/packs/demo/0.1.0/"),
            "baseline version serves"
        );
        drop(dir2);
    }
}
