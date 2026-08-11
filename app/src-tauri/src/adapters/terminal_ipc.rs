//! Translation between the webview's wire and the terminal core's vocabulary
//! (ADR "Byte transport across the application boundary"; design D3).
//!
//! Thin on purpose: bytes in, bytes out, and the platform question of *which* shell a
//! session starts. Every rule about who may be addressed and what a size may be lives in
//! `core::terminal`.

use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, Request};

use crate::core::terminal::{
    resolve_shell, Presenting, SessionError, SessionId, ShellConfig, Size, TerminalConfig,
};

/// Header carrying the session a raw-bodied write is addressed to.
///
/// The body of a `terminal_write` is the *bytes themselves* — that is the whole point of
/// the raw path — so there is nowhere in it to put the session. Tauri's documented answer
/// for metadata alongside a raw body is a header, and this is that.
pub const SESSION_HEADER: &str = "x-terminal-session";

/// Where a session's output goes: straight down a per-session channel as raw bytes.
///
/// `InvokeResponseBody::Raw` is what keeps this byte-transparent — the payload never
/// becomes JSON, so control sequences, split multi-byte characters and binary output all
/// survive (spec `terminal-session`). It is also why output does **not** travel on the app
/// event bus, which is JSON and carries domain facts.
pub fn output_sink(channel: Channel) -> impl Fn(&[u8]) + Send + 'static {
    move |bytes: &[u8]| {
        // A closed webview is the ordinary way this fails; the session's own exit path
        // reports the end, so there is nothing useful to do with the error here.
        let _ = channel.send(InvokeResponseBody::Raw(bytes.to_vec()));
    }
}

/// The session a raw-bodied request names.
pub fn addressed_session(request: &Request<'_>) -> Result<SessionId, SessionError> {
    let raw = request
        .headers()
        .get(SESSION_HEADER)
        .ok_or_else(|| SessionError::Io(format!("request carries no {SESSION_HEADER} header")))?
        .to_str()
        .map_err(|_| SessionError::Io(format!("{SESSION_HEADER} is not readable text")))?;
    raw.parse::<u64>()
        .map(SessionId::new)
        .map_err(|_| SessionError::Io(format!("{SESSION_HEADER} is not a session id: {raw}")))
}

/// The bytes a raw-bodied request carries.
///
/// A JSON body is refused rather than coerced: silently accepting one would mean the
/// byte-transparency guarantee held only for callers who happened to use the raw path.
pub fn raw_body<'a>(request: &'a Request<'a>) -> Result<&'a [u8], SessionError> {
    match request.body() {
        InvokeBody::Raw(bytes) => Ok(bytes),
        InvokeBody::Json(_) => Err(SessionError::Io(
            "terminal input must be sent as raw bytes, not JSON".into(),
        )),
    }
}

/// Which shell this platform should start, given the configured candidates.
///
/// The platform split lives here rather than in the core because "which family of
/// candidates applies" and "what `$SHELL` means" are both facts about the host.
pub fn shell_for_this_platform(config: &TerminalConfig) -> Result<String, SessionError> {
    let ShellConfig { windows, unix } = &config.shell;
    let candidates = if cfg!(windows) { windows } else { unix };
    // `$SHELL` is a Unix convention and is honoured only there.
    //
    // On Windows it must be ignored even when set, because it usually is: Git Bash, MSYS
    // and Cygwin all export it. Honouring it would mean the same binary starts PowerShell
    // when launched from Explorer and Git Bash when launched from a Git Bash prompt —
    // the terminal you get would depend on how the app happened to be started. Caught by
    // `no_candidate_yields_a_stated_reason_not_a_panic` on a developer machine.
    //
    // `%COMSPEC%` is deliberately not consulted either (core::terminal::config).
    let env_shell = if cfg!(windows) {
        None
    } else {
        std::env::var("SHELL").ok()
    };
    resolve_shell(candidates, env_shell.as_deref(), &|program| {
        which::which(program).is_ok()
    })
    .map_err(|e| SessionError::Spawn(e.to_string()))
}

/// Validate a requested size at the boundary, so the refusal reaches the caller as a
/// stated reason rather than a deserialization failure.
pub fn requested_size(columns: i64, rows: i64) -> Result<Size, SessionError> {
    Size::new(columns, rows).map_err(SessionError::from)
}

/// What the surface reports about the program it is presenting (design D3).
///
/// A boolean on the wire, a named thing in the core: the webview cannot be asked to know
/// what an alternate screen buffer implies for signal delivery, and the core must not be
/// reading booleans whose meaning lives in a comment.
pub fn presenting(full_screen: bool) -> Presenting {
    if full_screen {
        Presenting::FullScreen
    } else {
        Presenting::Normally
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(windows: &[&str], unix: &[&str]) -> TerminalConfig {
        serde_json::from_value(serde_json::json!({
            "shell": {
                "windows": windows.iter().collect::<Vec<_>>(),
                "unix": unix.iter().collect::<Vec<_>>(),
            }
        }))
        .expect("test config parses")
    }

    #[test]
    fn the_platform_picks_its_own_candidate_list() {
        // Whichever platform this runs on, a shell that exists there must be found —
        // and the other platform's list must not be consulted.
        let found = shell_for_this_platform(&config(
            &["cmd.exe"],
            &["/bin/sh", "/usr/bin/sh", "/bin/bash"],
        ));
        assert!(found.is_ok(), "this machine has a shell: {found:?}");
    }

    #[test]
    fn no_candidate_yields_a_stated_reason_not_a_panic() {
        let err = shell_for_this_platform(&config(
            &["steward-nonexistent.exe"],
            &["/steward/nonexistent"],
        ))
        .expect_err("nothing configured exists");
        match err {
            SessionError::Spawn(reason) => assert!(
                reason.contains("steward-nonexistent") || reason.contains("/steward/nonexistent"),
                "the reason names what was tried: {reason}"
            ),
            other => panic!("expected a spawn refusal, got {other:?}"),
        }
    }

    #[test]
    #[cfg(windows)]
    fn an_inherited_unix_shell_variable_is_ignored_on_windows() {
        // Git Bash, MSYS and Cygwin all export $SHELL on Windows. Honouring it would make
        // the terminal you get depend on how the app was launched.
        std::env::set_var("SHELL", "C:/Program Files/Git/bin/bash.exe");
        let chosen = shell_for_this_platform(&config(&["cmd.exe"], &["/bin/sh"]))
            .expect("cmd.exe exists on every Windows machine");
        assert!(
            chosen.to_ascii_lowercase().contains("cmd"),
            "the configured Windows candidate wins over an inherited $SHELL, got {chosen}"
        );
    }

    #[test]
    fn the_surfaces_observation_keeps_its_meaning_across_the_wire() {
        // Getting this the wrong way round would raise a control event at exactly the
        // full-screen programs that must never receive one.
        assert_eq!(presenting(true), Presenting::FullScreen);
        assert_eq!(presenting(false), Presenting::Normally);
    }

    #[test]
    fn a_degenerate_size_is_refused_at_the_boundary() {
        assert!(matches!(requested_size(0, 24), Err(SessionError::Size(_))));
        assert!(requested_size(80, 24).is_ok());
    }
}
