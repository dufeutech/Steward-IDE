//! Pack configuration vocabulary: what packs exist, what each is for, and whether the
//! binary embeds a copy of it (spec `baseline-boot`; design D3).
//!
//! Pure: these types parse and validate. Reading the file is the adapter's job.

use serde::Deserialize;

/// What a pack is for. Exactly one pack is the bootstrap: the embedded recovery surface
/// shown while no application pack can be served (spec `bootstrap-shell`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackRole {
    Application,
    Bootstrap,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackConfig {
    /// URL segment and store key, e.g. `xkin`.
    pub pack: String,
    /// Registry identifier, e.g. `pack:assets.xkin` (Rule 11, ADR D7).
    pub id: String,
    pub role: PackRole,
    /// Absent = the binary embeds no copy of this pack. It then resolves only from the
    /// store, and having no version at boot is not a fault — boot falls through to the
    /// bootstrap surface (spec `baseline-boot`).
    #[serde(default)]
    pub embedded_version: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    NoBootstrapPack,
    ManyBootstrapPacks(Vec<String>),
    BootstrapNotEmbedded(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoBootstrapPack => write!(
                f,
                "no pack declares role \"bootstrap\": nothing could be served on a fresh install"
            ),
            Self::ManyBootstrapPacks(names) => write!(
                f,
                "{} packs declare role \"bootstrap\" ({}): exactly one may",
                names.len(),
                names.join(", ")
            ),
            Self::BootstrapNotEmbedded(name) => write!(
                f,
                "bootstrap pack \"{name}\" declares no embedded_version: it could not be served \
                 when nothing has been downloaded"
            ),
        }
    }
}

/// Fail at load rather than at the first unresolvable pack: a config that cannot boot
/// should say so at startup, not present a blank window later (design D3).
pub fn validate_packs(packs: &[PackConfig]) -> Result<(), ConfigError> {
    let bootstraps: Vec<&PackConfig> = packs
        .iter()
        .filter(|p| p.role == PackRole::Bootstrap)
        .collect();
    match bootstraps.as_slice() {
        [] => Err(ConfigError::NoBootstrapPack),
        [one] => {
            if one.embedded_version.is_none() {
                Err(ConfigError::BootstrapNotEmbedded(one.pack.clone()))
            } else {
                Ok(())
            }
        }
        many => Err(ConfigError::ManyBootstrapPacks(
            many.iter().map(|p| p.pack.clone()).collect(),
        )),
    }
}

/// The bootstrap pack. `validate_packs` guarantees exactly one exists.
pub fn bootstrap(packs: &[PackConfig]) -> Option<&PackConfig> {
    packs.iter().find(|p| p.role == PackRole::Bootstrap)
}

/// Application packs, in configuration order.
pub fn applications(packs: &[PackConfig]) -> impl Iterator<Item = &PackConfig> {
    packs.iter().filter(|p| p.role == PackRole::Application)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(name: &str, role: PackRole, embedded: Option<&str>) -> PackConfig {
        PackConfig {
            pack: name.into(),
            id: format!("pack:assets.{name}"),
            role,
            embedded_version: embedded.map(Into::into),
        }
    }

    #[test]
    fn scenario_one_embedded_bootstrap_is_valid() {
        let packs = [
            pack("bootstrap", PackRole::Bootstrap, Some("0.1.0")),
            pack("xkin", PackRole::Application, None),
        ];
        assert_eq!(validate_packs(&packs), Ok(()));
        assert_eq!(
            bootstrap(&packs).map(|p| p.pack.as_str()),
            Some("bootstrap")
        );
        assert_eq!(applications(&packs).count(), 1);
    }

    #[test]
    fn scenario_no_bootstrap_pack_is_refused() {
        let packs = [pack("xkin", PackRole::Application, None)];
        assert_eq!(validate_packs(&packs), Err(ConfigError::NoBootstrapPack));
    }

    #[test]
    fn scenario_two_bootstrap_packs_are_refused() {
        let packs = [
            pack("a", PackRole::Bootstrap, Some("0.1.0")),
            pack("b", PackRole::Bootstrap, Some("0.1.0")),
        ];
        assert_eq!(
            validate_packs(&packs),
            Err(ConfigError::ManyBootstrapPacks(vec![
                "a".into(),
                "b".into()
            ]))
        );
    }

    #[test]
    fn scenario_bootstrap_without_embedded_version_is_refused() {
        let packs = [pack("bootstrap", PackRole::Bootstrap, None)];
        assert_eq!(
            validate_packs(&packs),
            Err(ConfigError::BootstrapNotEmbedded("bootstrap".into()))
        );
    }

    #[test]
    fn scenario_application_pack_needs_no_embedded_version() {
        let json = serde_json::json!({
            "pack": "xkin", "id": "pack:assets.xkin", "role": "application"
        });
        let parsed: PackConfig = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.role, PackRole::Application);
        assert!(parsed.embedded_version.is_none());
    }

    #[test]
    fn scenario_unknown_field_is_refused() {
        // A stale `baseline_version` must fail loudly rather than be ignored, which
        // would silently drop a pack's embedded copy.
        let json = serde_json::json!({
            "pack": "xkin", "id": "pack:assets.xkin", "role": "application",
            "baseline_version": "0.1.0"
        });
        assert!(serde_json::from_value::<PackConfig>(json).is_err());
    }
}
