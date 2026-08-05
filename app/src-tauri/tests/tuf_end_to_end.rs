//! End-to-end verification against a real signed repository (spec `pack-publish`).
//!
//! Every other test in this crate stubs the update source. These do not: they load the
//! committed fixture — produced by the actual publisher pipeline — through the real
//! `TufSource`, so publisher output and client expectations are checked against each
//! other rather than against a shared assumption. That mismatch is exactly what these
//! tests exist to catch; the fixture's flat target names were wrong the first time.
//!
//! `tough` serves `file://` URLs through its default transport, so no server and no
//! secrets are involved. One test is the exception and says so: it dials a refused
//! loopback port to prove https is a transportable scheme at all, which `file://` alone
//! can never establish. Nothing here reaches the internet.

use std::path::{Path, PathBuf};

use steward_ide_lib::adapters::fs_store::FsStore;
use steward_ide_lib::adapters::tuf_source::TufSource;
use steward_ide_lib::adapters::updater::{run_update, UpdateOutcome};

const PACK: &str = "fixture";
const FIXTURE_VERSION: &str = "1.0.0";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tuf-repo")
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

/// `file://` URL for a directory, with the trailing slash `tough` expects when it joins
/// relative metadata and target names onto the base.
fn dir_url(path: &Path) -> String {
    let url = url_from_path(path);
    if url.ends_with('/') {
        url
    } else {
        format!("{url}/")
    }
}

fn url_from_path(path: &Path) -> String {
    // `Url::from_directory_path` handles Windows drive letters and percent-encoding.
    url::Url::from_directory_path(path)
        .expect("fixture path is absolute")
        .to_string()
}

async fn load_source(repo: &Path, datastore: &Path) -> Result<TufSource, String> {
    let root = std::fs::read(repo.join("root.json")).expect("fixture root.json");
    TufSource::load(
        &root,
        &dir_url(&repo.join("metadata")),
        &dir_url(&repo.join("targets")),
        datastore,
        PACK,
    )
    .await
}

/// Copy the fixture so a test can corrupt it without touching the committed tree.
fn fixture_copy(into: &Path) -> PathBuf {
    let repo = into.join("repo");
    copy_tree(&fixture_dir(), &repo);
    repo
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn find_target(repo: &Path, suffix: &str) -> PathBuf {
    std::fs::read_dir(repo.join("targets"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.file_name().unwrap().to_string_lossy().ends_with(suffix))
        .unwrap_or_else(|| panic!("no target ending in {suffix}"))
}

#[tokio::test]
async fn scenario_client_accepts_what_the_publisher_produced() {
    let temp = tempfile::tempdir().unwrap();
    let store = FsStore::open(temp.path().join("store")).unwrap();

    let source = load_source(&fixture_dir(), &temp.path().join("datastore"))
        .await
        .expect("fixture repository verifies against its own root");

    let outcome = run_update(&store, &schema(), &source, PACK).await;

    assert_eq!(
        outcome,
        UpdateOutcome::Activated {
            version: FIXTURE_VERSION.to_string()
        },
        "a valid signed release must verify, download, and become activatable"
    );
}

#[tokio::test]
async fn scenario_tampered_blob_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let repo = fixture_copy(temp.path());
    let store = FsStore::open(temp.path().join("store")).unwrap();

    // Rewrite a content blob after signing. Its bytes no longer hash to the name the
    // signed metadata gives it.
    let blob = find_target(&repo, "fixture.manifest.json");
    let victim = std::fs::read_dir(repo.join("targets"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| *p != blob)
        .expect("a content blob besides the manifest");
    std::fs::write(&victim, b"tampered").unwrap();

    let source = load_source(&repo, &temp.path().join("datastore")).await;
    let outcome = match source {
        // Rejection may surface at load or at fetch, depending on which file was hit;
        // either way nothing may be activated.
        Err(_) => UpdateOutcome::Failed {
            reason: "repository rejected at load".into(),
        },
        Ok(source) => run_update(&store, &schema(), &source, PACK).await,
    };

    assert!(
        matches!(outcome, UpdateOutcome::Failed { .. }),
        "a tampered blob must not produce an activation, got {outcome:?}"
    );
    assert!(
        store.active_version(PACK).unwrap().is_none(),
        "nothing may be activated when verification fails"
    );
}

#[tokio::test]
async fn scenario_tampered_metadata_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let repo = fixture_copy(temp.path());

    // Bump the version inside signed metadata: valid JSON, invalid signature.
    let timestamp = repo.join("metadata/timestamp.json");
    let mut doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&timestamp).unwrap()).unwrap();
    doc["signed"]["version"] = serde_json::json!(99);
    std::fs::write(&timestamp, serde_json::to_vec(&doc).unwrap()).unwrap();

    let result = load_source(&repo, &temp.path().join("datastore")).await;

    assert!(
        result.is_err(),
        "metadata whose signature does not cover its contents must be refused"
    );
}

#[test]
fn fixture_root_expiry_stays_far_in_the_future() {
    let root: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture_dir().join("root.json")).unwrap()).unwrap();
    let expires = root["signed"]["expires"].as_str().expect("expires field");

    // Guards against the fixture quietly becoming a time bomb: tests must fail for
    // code reasons, never because a date passed.
    assert!(
        expires > "2100",
        "fixture root expires at {expires}; regenerate it with a far-future expiry"
    );
}

#[tokio::test]
async fn https_endpoints_are_a_supported_scheme() {
    // Every test above loads over `file://`, which needs no cargo feature. Production
    // loads over https, which needs `tough`'s `http` feature — off by default. Without
    // it the updater failed against the real endpoint with "unsupported URL scheme"
    // while the whole fixture suite stayed green.
    //
    // Port 1 on loopback refuses immediately: no DNS, no egress, no flakiness. The
    // connection is *expected* to fail; what matters is how.
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::read(fixture_dir().join("root.json")).unwrap();

    let result = TufSource::load(
        &root,
        "https://127.0.0.1:1/metadata/",
        "https://127.0.0.1:1/targets/",
        &temp.path().join("datastore"),
        PACK,
    )
    .await;
    // `TufSource` is not Debug, so unwrap the error by hand rather than expect_err.
    let err = match result {
        Ok(_) => panic!("nothing is listening on port 1"),
        Err(e) => e,
    };

    assert!(
        !err.contains("unsupported URL scheme") && !err.contains("http feature"),
        "https must be a transportable scheme — enable tough's `http` feature. Got: {err}"
    );
}
