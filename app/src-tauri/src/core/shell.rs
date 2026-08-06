//! Shell tag generation (spec `pack-manifest`: entry tags are generated, never
//! hand-written). Pure string work: manifest in, HTML fragments out.

use super::config::{self, PackConfig};
use super::manifest::Manifest;

/// Escape the characters that matter inside a double-quoted HTML attribute.
fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

/// Generate `<script>`/`<link>` tags for a pack's entry points, in manifest order,
/// under `base` (e.g. `/packs/xkin/0.1.0`). External references only — the spike
/// proved inline scripts are dead under the enforced CSP.
pub fn entry_tags(manifest: &Manifest, base: &str) -> (String, String) {
    let styles = manifest
        .entry
        .styles
        .iter()
        .map(|p| {
            format!(
                r#"<link rel="stylesheet" href="{}/{}" />"#,
                base,
                attr_escape(p)
            )
        })
        .collect::<Vec<_>>()
        .join("\n    ");
    let scripts = manifest
        .entry
        .scripts
        .iter()
        .map(|p| format!(r#"<script src="{}/{}"></script>"#, base, attr_escape(p)))
        .collect::<Vec<_>>()
        .join("\n    ");
    (styles, scripts)
}

/// Substitute generated tags into the shell template. Placeholders are literal
/// `%%STYLE_TAGS%%`, `%%SCRIPT_TAGS%%`, and `%%COMPOSITION%%`.
///
/// The composition marker tells the shell which surface it is hosting, so it never has to
/// infer that from the presence of some pack's global (design D2).
pub fn render_shell(template: &str, styles: &str, scripts: &str, composition: &str) -> String {
    template
        .replace("%%STYLE_TAGS%%", styles)
        .replace("%%SCRIPT_TAGS%%", scripts)
        .replace("%%COMPOSITION%%", composition)
}

/// Which packs' entry tags compose the page (design D2).
///
/// Application packs when *every* application pack resolved — a page missing part of the
/// application is not a working application — and the bootstrap pack otherwise. Selection
/// lives here, not in resolution, so no baseline-specific branch enters the serving path
/// (spec `baseline-boot`).
#[derive(Debug, PartialEq, Eq)]
pub enum Composition<'a> {
    Application(Vec<&'a PackConfig>),
    Bootstrap(&'a PackConfig),
    /// Not even the embedded surface resolved: the binary's own content is unusable.
    Nothing,
}

impl Composition<'_> {
    /// The marker the shell template carries, so the surface knows what it is hosting.
    pub fn marker(&self) -> &'static str {
        match self {
            Self::Application(_) => "application",
            Self::Bootstrap(_) => "bootstrap",
            Self::Nothing => "none",
        }
    }
}

pub fn compose<'a>(packs: &'a [PackConfig], resolved: &dyn Fn(&str) -> bool) -> Composition<'a> {
    let apps: Vec<&PackConfig> = config::applications(packs).collect();
    if !apps.is_empty() && apps.iter().all(|p| resolved(&p.pack)) {
        return Composition::Application(apps);
    }
    match config::bootstrap(packs) {
        Some(boot) if resolved(&boot.pack) => Composition::Bootstrap(boot),
        _ => Composition::Nothing,
    }
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
                {"path": "dist/a.js", "size": 1, "sha256": "a".repeat(64)},
                {"path": "dist/b.js", "size": 1, "sha256": "b".repeat(64)},
                {"path": "dist/s.css", "size": 1, "sha256": "c".repeat(64)}
            ],
            "entry": {"scripts": ["dist/a.js", "dist/b.js"], "styles": ["dist/s.css"]}
        }))
        .unwrap()
    }

    #[test]
    fn scenario_tags_generated_in_manifest_order() {
        let (styles, scripts) = entry_tags(&manifest(), "/packs/xkin/0.1.0");
        assert!(styles.contains(r#"href="/packs/xkin/0.1.0/dist/s.css""#));
        let a = scripts.find("dist/a.js").unwrap();
        let b = scripts.find("dist/b.js").unwrap();
        assert!(a < b, "script order follows the manifest");
    }

    #[test]
    fn render_substitutes_every_placeholder() {
        let html = render_shell(
            r#"<body data-composition="%%COMPOSITION%%"><head>%%STYLE_TAGS%%</head>%%SCRIPT_TAGS%%</body>"#,
            "S",
            "J",
            "application",
        );
        assert_eq!(
            html,
            r#"<body data-composition="application"><head>S</head>J</body>"#
        );
    }

    fn packs() -> Vec<PackConfig> {
        use config::PackRole;
        vec![
            PackConfig {
                pack: "xkin".into(),
                id: "pack:assets.xkin".into(),
                role: PackRole::Application,
                embedded_version: None,
            },
            PackConfig {
                pack: "bootstrap".into(),
                id: "pack:assets.bootstrap".into(),
                role: PackRole::Bootstrap,
                embedded_version: Some("0.1.0".into()),
            },
        ]
    }

    #[test]
    fn scenario_all_application_packs_resolve() {
        let packs = packs();
        let composition = compose(&packs, &|_| true);
        match composition {
            Composition::Application(chosen) => {
                assert_eq!(
                    chosen.len(),
                    1,
                    "the bootstrap pack does not compose the page"
                );
                assert_eq!(chosen[0].pack, "xkin");
            }
            other => panic!("expected the application, got {other:?}"),
        }
        assert_eq!(compose(&packs, &|_| true).marker(), "application");
    }

    #[test]
    fn scenario_no_application_pack_resolves() {
        let packs = packs();
        let composition = compose(&packs, &|p| p == "bootstrap");
        assert_eq!(composition, Composition::Bootstrap(&packs[1]));
        assert_eq!(composition.marker(), "bootstrap");
    }

    #[test]
    fn scenario_one_of_several_application_packs_is_unresolved() {
        // A page missing part of the application is not a working application: the
        // bootstrap surface is the honest answer, not a half-composed page.
        let mut packs = packs();
        packs.insert(
            1,
            PackConfig {
                pack: "other".into(),
                id: "pack:assets.other".into(),
                role: config::PackRole::Application,
                embedded_version: None,
            },
        );
        let composition = compose(&packs, &|p| p != "other");
        assert_eq!(composition.marker(), "bootstrap");
    }

    #[test]
    fn scenario_bootstrap_pack_also_unresolved() {
        // The binary's own embedded content is unusable — distinct from "nothing has been
        // downloaded yet", which is the bootstrap case above.
        let packs = packs();
        assert_eq!(compose(&packs, &|_| false), Composition::Nothing);
    }

    #[test]
    fn scenario_no_application_packs_configured() {
        let packs = vec![packs().remove(1)];
        assert_eq!(compose(&packs, &|_| true).marker(), "bootstrap");
    }
}
