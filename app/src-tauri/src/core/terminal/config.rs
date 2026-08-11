//! Terminal configuration vocabulary (spec `terminal-session`, `terminal-surface`).
//!
//! Pure: these types parse and decide. Reading the file and probing the filesystem are
//! the adapter's job — `resolve_shell` takes the environment and an existence test as
//! arguments so the decision is testable with nothing installed.

use serde::{Deserialize, Serialize};

/// Which shell a session starts, and how much scrollback the surface retains.
///
/// Both are config rather than literals in code: the shell because it is the single most
/// machine-specific thing here, the scrollback bound because `terminal-surface` requires
/// a *stated* bound and a number buried in a bundle is not stated anywhere.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalConfig {
    #[serde(default = "default_scrollback")]
    pub scrollback_lines: u32,
    pub shell: ShellConfig,
}

fn default_scrollback() -> u32 {
    1000
}

/// Candidate programs, most-preferred first, per platform family.
///
/// A list rather than one name because the right shell differs per machine, not per
/// project: `pwsh` is present on a developer's Windows box and absent on a stock one, and
/// falling back is better than failing to open a terminal at all.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellConfig {
    pub windows: Vec<String>,
    pub unix: Vec<String>,
}

/// What the surface needs to know at startup. A query (Rule 10: queries return data and
/// change nothing) — deliberately not the whole config, so the webview learns only what
/// it presents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SurfaceConfig {
    pub scrollback_lines: u32,
}

impl From<&TerminalConfig> for SurfaceConfig {
    fn from(c: &TerminalConfig) -> Self {
        Self {
            scrollback_lines: c.scrollback_lines,
        }
    }
}

/// Pick the program a session starts.
///
/// The user's own shell wins when the environment names one and it is present — a session
/// should behave like a terminal the user opened themselves (spec `terminal-session`:
/// "Commands see the user's environment"). Only when that is absent or gone do the
/// configured candidates apply, in order.
///
/// `env_shell` is `$SHELL` on Unix. It is deliberately **not** `%COMSPEC%` on Windows:
/// `%COMSPEC%` is `cmd.exe` on essentially every machine, which would make the worst
/// available shell the default one everywhere (design Open Question 1).
pub fn resolve_shell(
    candidates: &[String],
    env_shell: Option<&str>,
    exists: &dyn Fn(&str) -> bool,
) -> Result<String, NoShell> {
    if let Some(shell) = env_shell.filter(|s| !s.is_empty()).filter(|s| exists(s)) {
        return Ok(shell.to_string());
    }
    candidates
        .iter()
        .find(|c| exists(c))
        .cloned()
        .ok_or_else(|| NoShell {
            tried: candidates.to_vec(),
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoShell {
    pub tried: Vec<String>,
}

impl std::fmt::Display for NoShell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.tried.is_empty() {
            return write!(f, "no shell candidates are configured for this platform");
        }
        write!(
            f,
            "none of the configured shells exist: {}",
            self.tried.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn present(names: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |p: &str| names.contains(&p)
    }

    #[test]
    fn the_users_own_shell_wins_when_present() {
        let found = resolve_shell(
            &candidates(&["/bin/sh"]),
            Some("/usr/bin/fish"),
            &present(&["/usr/bin/fish", "/bin/sh"]),
        );
        assert_eq!(found.unwrap(), "/usr/bin/fish");
    }

    #[test]
    fn a_stale_env_shell_falls_through_rather_than_failing() {
        // $SHELL naming an uninstalled shell must not be the reason a terminal refuses
        // to open.
        let found = resolve_shell(
            &candidates(&["/bin/bash", "/bin/sh"]),
            Some("/usr/bin/removed"),
            &present(&["/bin/sh"]),
        );
        assert_eq!(found.unwrap(), "/bin/sh");
    }

    #[test]
    fn candidates_are_tried_in_configured_order() {
        let found = resolve_shell(
            &candidates(&["pwsh.exe", "powershell.exe"]),
            None,
            &present(&["pwsh.exe", "powershell.exe"]),
        );
        assert_eq!(
            found.unwrap(),
            "pwsh.exe",
            "the first present candidate wins"
        );
    }

    #[test]
    fn scenario_the_shell_cannot_be_started() {
        let err = resolve_shell(&candidates(&["pwsh.exe"]), None, &present(&[])).unwrap_err();
        // The reason names what was tried — a surface has to be able to say why.
        assert!(err.to_string().contains("pwsh.exe"));
    }

    #[test]
    fn an_empty_env_shell_is_ignored() {
        let found = resolve_shell(&candidates(&["/bin/sh"]), Some(""), &present(&["/bin/sh"]));
        assert_eq!(found.unwrap(), "/bin/sh");
    }

    #[test]
    fn scrollback_has_a_stated_default() {
        let parsed: TerminalConfig = serde_json::from_value(serde_json::json!({
            "shell": { "windows": ["pwsh.exe"], "unix": ["/bin/sh"] }
        }))
        .unwrap();
        assert_eq!(parsed.scrollback_lines, 1000);
        assert_eq!(SurfaceConfig::from(&parsed).scrollback_lines, 1000);
    }

    #[test]
    fn an_unknown_field_is_refused() {
        // A misspelled key must fail loudly rather than silently leaving the default in
        // place — the same posture `PackConfig` takes.
        let json = serde_json::json!({
            "scrollback": 500,
            "shell": { "windows": [], "unix": [] }
        });
        assert!(serde_json::from_value::<TerminalConfig>(json).is_err());
    }
}
