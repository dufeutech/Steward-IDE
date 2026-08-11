//! Session vocabulary and the `Pty` port (spec `terminal-session`; design D2/D4).
//!
//! Pure: identifiers, sizes, exit causes, and the interface the OS-facing adapter
//! implements. Nothing here spawns a process or names a concrete PTY library.

use std::fmt;

/// Opaque session handle. Unique for the application's lifetime and never reused, so a
/// stale identifier can never be mistaken for a live session (spec `terminal-session`:
/// "addressed explicitly and never ambiently").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(u64);

impl SessionId {
    /// Construction is deliberately crate-private: identifiers come from `Registry`'s
    /// counter, never from the webview or from arithmetic at a call site.
    pub(crate) fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A terminal's visible extent, in character cells.
///
/// Constructed only through [`Size::new`], so a size that reached this type is a size a
/// shell can be told about — there is no way to build a zero-column terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub columns: u16,
    pub rows: u16,
}

impl Size {
    /// Accepts the wire's `i64` rather than `u16` on purpose: a negative or oversized
    /// count must be *refused by us*, with a reason, rather than rejected by the
    /// deserializer as a malformed request (spec scenario "A degenerate size is
    /// requested").
    pub fn new(columns: i64, rows: i64) -> Result<Self, SizeRejected> {
        let check = |value: i64, axis: &'static str| match value {
            v if v <= 0 => Err(SizeRejected { axis, value }),
            v if v > u16::MAX as i64 => Err(SizeRejected { axis, value }),
            v => Ok(v as u16),
        };
        Ok(Self {
            columns: check(columns, "columns")?,
            rows: check(rows, "rows")?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeRejected {
    pub axis: &'static str,
    pub value: i64,
}

impl fmt::Display for SizeRejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} must be between 1 and {}, got {}",
            self.axis,
            u16::MAX,
            self.value
        )
    }
}

/// Why a session stopped. The three arms are kept distinct because a surface must be able
/// to tell a clean exit from a kill from a session that never worked — the same reasoning
/// that keeps `transport`/`verification`/`local` apart in the acquisition events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitCause {
    /// The shell exited on its own.
    Exited { code: i32 },
    /// The shell was terminated rather than exiting.
    ///
    /// The signal is a *name* (`"SIGKILL"`), not a number: Windows has no signals at all,
    /// and the numbering differs across Unix platforms, so a number would be meaningless
    /// on the wire without also carrying the platform.
    Signalled { signal: Option<String> },
    /// The session itself failed — the shell never ran, or its PTY broke under it.
    Failed { reason: String },
}

impl ExitCause {
    /// The wire tag (design D8). Mirrors how `acquisitionFailure.kind` is serialized.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Exited { .. } => "exited",
            Self::Signalled { .. } => "signalled",
            Self::Failed { .. } => "failed",
        }
    }

    /// The exit status, where one exists. `None` for a kill or a failure — absent, not
    /// zero, because zero already means "exited successfully".
    pub fn code(&self) -> Option<i32> {
        match self {
            Self::Exited { code } => Some(*code),
            Self::Signalled { .. } | Self::Failed { .. } => None,
        }
    }

    /// Human-readable detail: the signal name for a kill, the reason for a failure.
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Exited { .. } => None,
            Self::Signalled { signal } => signal.as_deref(),
            Self::Failed { reason } => Some(reason),
        }
    }
}

impl fmt::Display for ExitCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exited { code } => write!(f, "exited with status {code}"),
            Self::Signalled { signal: Some(s) } => write!(f, "terminated by {s}"),
            Self::Signalled { signal: None } => write!(f, "terminated"),
            Self::Failed { reason } => write!(f, "failed: {reason}"),
        }
    }
}

/// Everything that can go wrong addressing a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    /// An identifier that was never issued. Distinct from `Ended` so a caller can tell a
    /// bug from a race with a shell that just exited.
    Unknown(SessionId),
    Ended(SessionId),
    Size(SizeRejected),
    /// No shell could be started (spec scenario "The shell cannot be started").
    Spawn(String),
    /// The session exists and is live, but the operation failed at the boundary.
    Io(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(id) => write!(f, "no session {id}"),
            Self::Ended(id) => write!(f, "session {id} has ended"),
            Self::Size(e) => write!(f, "{e}"),
            Self::Spawn(reason) => write!(f, "no shell could be started: {reason}"),
            Self::Io(reason) => write!(f, "session i/o failed: {reason}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<SizeRejected> for SessionError {
    fn from(e: SizeRejected) -> Self {
        Self::Size(e)
    }
}

/// Where a live session's output goes. The core never decides what happens to bytes; it
/// only guarantees they arrive in the order the shell produced them.
pub type OutputSink = Box<dyn Fn(&[u8]) + Send + 'static>;

/// Called once, when the shell backing a session stops, with why.
pub type ExitSink = Box<dyn FnOnce(ExitCause) + Send + 'static>;

/// How a session is started. Named separately from [`Pty`] because starting is the part
/// that needs configuration and the part that can fail before anything exists.
pub struct SpawnRequest {
    /// Program to run, already resolved to something on disk (see `config::resolve_shell`).
    pub program: String,
    pub size: Size,
    pub on_output: OutputSink,
    pub on_exit: ExitSink,
}

/// Port (Rule 2): the OS-facing side of one live session.
///
/// Defined in the core, implemented in `adapters::pty`. No implementation type crosses
/// back — the core holds `Box<dyn Pty>` and nothing more, so the PTY library is swappable
/// by editing one file (ADR: PTY control).
pub trait Pty: Send {
    fn write(&mut self, bytes: &[u8]) -> Result<(), SessionError>;
    fn resize(&mut self, size: Size) -> Result<(), SessionError>;
    /// Interrupt whatever the session is running, leaving the session itself alive.
    ///
    /// Distinct from `write` because it is not input: it is an operation the specification
    /// names, with a refusal of its own when the session is unknown or ended. Turning it
    /// into a byte is the adapter's business, and how that byte is honoured — a signal for
    /// the running command, or input for a program that took raw control — is the
    /// platform's (design D2b, D4).
    fn interrupt(&mut self) -> Result<(), SessionError>;
    /// Terminate the shell and release the session's resources. Idempotent: closing an
    /// already-closed session is not an error, because the shell may have exited first.
    fn close(&mut self) -> Result<(), SessionError>;
}

/// Port (Rule 2): what starts sessions.
pub trait PtySpawner: Send + Sync {
    fn spawn(&self, request: SpawnRequest) -> Result<Box<dyn Pty>, SessionError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_a_degenerate_size_is_requested() {
        for (columns, rows, axis) in [(0, 24, "columns"), (80, 0, "rows"), (-1, 24, "columns")] {
            let rejected = Size::new(columns, rows).expect_err("degenerate size must be refused");
            assert_eq!(rejected.axis, axis);
        }
    }

    #[test]
    fn oversized_dimensions_are_refused_rather_than_wrapping() {
        // Silently truncating 65_537 to 1 would tell the shell a size nobody asked for.
        assert!(Size::new(65_536, 24).is_err());
        assert_eq!(
            Size::new(65_535, 24).unwrap(),
            Size {
                columns: 65_535,
                rows: 24
            }
        );
    }

    #[test]
    fn ordinary_sizes_are_accepted() {
        assert_eq!(
            Size::new(80, 24).unwrap(),
            Size {
                columns: 80,
                rows: 24
            }
        );
    }

    #[test]
    fn exit_causes_stay_distinguishable_on_the_wire() {
        assert_eq!(ExitCause::Exited { code: 0 }.tag(), "exited");
        assert_eq!(ExitCause::Exited { code: 2 }.code(), Some(2));
        let killed = ExitCause::Signalled {
            signal: Some("SIGKILL".into()),
        };
        assert_eq!(killed.tag(), "signalled");
        assert_eq!(killed.detail(), Some("SIGKILL"));
        // Absent, not zero: zero already means "exited successfully".
        assert_eq!(killed.code(), None);
        assert_eq!(
            ExitCause::Failed {
                reason: "pty broke".into()
            }
            .tag(),
            "failed"
        );
        // A clean exit and a kill must never render as the same fact.
        assert_ne!(
            ExitCause::Exited { code: 0 }.tag(),
            ExitCause::Signalled { signal: None }.tag()
        );
    }
}
