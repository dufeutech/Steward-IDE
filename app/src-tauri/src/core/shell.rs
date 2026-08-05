//! Shell tag generation (spec `pack-manifest`: entry tags are generated, never
//! hand-written). Pure string work: manifest in, HTML fragments out.

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
/// `%%STYLE_TAGS%%` and `%%SCRIPT_TAGS%%`.
pub fn render_shell(template: &str, styles: &str, scripts: &str) -> String {
    template
        .replace("%%STYLE_TAGS%%", styles)
        .replace("%%SCRIPT_TAGS%%", scripts)
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
    fn render_substitutes_both_placeholders() {
        let html = render_shell(
            "<head>%%STYLE_TAGS%%</head><body>%%SCRIPT_TAGS%%</body>",
            "S",
            "J",
        );
        assert_eq!(html, "<head>S</head><body>J</body>");
    }
}
