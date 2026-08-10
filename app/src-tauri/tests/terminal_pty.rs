//! The PTY adapter against a real shell (spec `terminal-session`).
//!
//! The core's rules are unit-tested against a fake `Pty`; what a fake cannot tell us is
//! whether a terminal was actually allocated, whether bytes survive the round trip
//! through it, and whether an exit is noticed. Those need a real process, so they live
//! here.
//!
//! **Why this harness answers a query.** ConPTY opens by asking the terminal where the
//! cursor is (`ESC[6n`) and *blocks until something answers*. In the product xterm.js
//! answers it; in a test there is no emulator, so without the answerback below a Windows
//! shell never reaches its prompt and every test here times out looking like a PTY bug.
//! Measured, not assumed — see the design's spike finding.
//!
//! Deliberately shell-agnostic otherwise: `echo` and `exit` are the two commands
//! `cmd.exe` and `sh` agree on, so one test body covers both platforms.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use steward_ide_lib::adapters::pty::NativePtySpawner;
use steward_ide_lib::core::terminal::{
    ExitCause, Pty, PtySpawner, SessionError, Size, SpawnRequest,
};

/// Generous: a first shell start on a cold Windows box is not fast, and a flaky timeout
/// would be worse than a slow test.
const LIMIT: Duration = Duration::from_secs(30);

const CURSOR_QUERY: &[u8] = b"\x1b[6n";
const CURSOR_ANSWER: &[u8] = b"\x1b[1;1R";

fn a_shell() -> String {
    if cfg!(windows) {
        "cmd.exe".into()
    } else {
        "/bin/sh".into()
    }
}

/// A live session plus everything the test needs to observe it.
struct Session {
    pty: Arc<Mutex<Option<Box<dyn Pty>>>>,
    output: Arc<Mutex<Vec<u8>>>,
    exited: mpsc::Receiver<ExitCause>,
}

impl Session {
    fn start(size: Size) -> Self {
        let output: Arc<Mutex<Vec<u8>>> = Arc::default();
        let pty: Arc<Mutex<Option<Box<dyn Pty>>>> = Arc::default();
        let (tx, exited) = mpsc::channel();

        let collected = Arc::clone(&output);
        let answering = Arc::clone(&pty);
        let started = NativePtySpawner::new(std::env::temp_dir())
            .spawn(SpawnRequest {
                program: a_shell(),
                size,
                on_output: Box::new(move |bytes| {
                    collected.lock().expect("poisoned").extend_from_slice(bytes);
                    if bytes.windows(CURSOR_QUERY.len()).any(|w| w == CURSOR_QUERY) {
                        if let Some(p) = answering.lock().expect("poisoned").as_mut() {
                            let _ = p.write(CURSOR_ANSWER);
                        }
                    }
                }),
                on_exit: Box::new(move |cause| {
                    let _ = tx.send(cause);
                }),
            })
            .unwrap_or_else(|e| panic!("a session must start on this machine: {e}"));

        *pty.lock().expect("poisoned") = Some(started);
        Self {
            pty,
            output,
            exited,
        }
    }

    fn write(&self, bytes: &[u8]) -> Result<(), SessionError> {
        self.pty
            .lock()
            .expect("poisoned")
            .as_mut()
            .expect("session is live")
            .write(bytes)
    }

    fn resize(&self, size: Size) -> Result<(), SessionError> {
        self.pty
            .lock()
            .expect("poisoned")
            .as_mut()
            .expect("session is live")
            .resize(size)
    }

    fn close(&self) -> Result<(), SessionError> {
        self.pty
            .lock()
            .expect("poisoned")
            .as_mut()
            .expect("session is live")
            .close()
    }

    fn wait_for_exit(&self) -> ExitCause {
        self.exited
            .recv_timeout(LIMIT)
            .expect("the shell must be reported as ended")
    }

    fn raw(&self) -> Vec<u8> {
        self.output.lock().expect("poisoned").clone()
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.raw()).into_owned()
    }

    /// Block until the shell stops producing output.
    ///
    /// An interactive shell drops anything typed before it is ready to read, so writing
    /// the instant the session opens loses the command and the test fails looking like a
    /// transport bug. There is no readiness signal to wait on — a prompt is just bytes,
    /// and differs per shell — so quiescence is the available proxy.
    fn wait_until_quiet(&self) {
        let deadline = std::time::Instant::now() + LIMIT;
        let mut last = usize::MAX;
        while std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(250));
            let seen = self.raw().len();
            if seen > 0 && seen == last {
                return;
            }
            last = seen;
        }
        panic!("the shell never settled; got:\n{}", self.text());
    }

    /// Run a command, let its output land, then let the shell exit on its own.
    fn run_then_exit(&self, command: &[u8]) -> ExitCause {
        self.wait_until_quiet();
        self.write(command).unwrap();
        self.write(b"\r\n").unwrap();
        self.wait_until_quiet();
        self.write(b"exit\r\n").unwrap();
        self.wait_for_exit()
    }
}

#[test]
fn scenario_a_session_is_started_and_carries_bytes_both_ways() {
    let session = Session::start(Size::new(80, 24).unwrap());

    // A marker rather than a common word: it must be something that cannot appear in a
    // shell banner by coincidence, or the test would pass without the round trip.
    let cause = session.run_then_exit(b"echo STEWARD_PTY_ROUNDTRIP_OK");

    let seen = session.text();
    assert!(
        seen.contains("STEWARD_PTY_ROUNDTRIP_OK"),
        "input reached the shell and its output came back; got:\n{seen}"
    );
    assert_eq!(
        cause.tag(),
        "exited",
        "a shell that exits on its own is reported as exited, not signalled: {cause}"
    );
    assert_eq!(cause.code(), Some(0));
}

#[test]
fn scenario_size_is_established_at_start() {
    // Proof that a real terminal was allocated rather than a pair of pipes: only a TTY
    // reports a size back. `mode con` and `stty size` are each platform's way to ask.
    let session = Session::start(Size::new(101, 37).unwrap());
    let ask: &[u8] = if cfg!(windows) {
        b"mode con"
    } else {
        b"stty size"
    };
    session.run_then_exit(ask);

    let seen = session.text();
    assert!(
        seen.contains("37"),
        "the shell must see the size the session was opened with; got:\n{seen}"
    );
}

#[test]
fn scenario_the_viewport_is_resized() {
    // Resizing a live session must be accepted by the platform, not merely by our types.
    let session = Session::start(Size::new(80, 24).unwrap());
    session.resize(Size::new(132, 43).unwrap()).unwrap();
    session.run_then_exit(b"echo RESIZED");
    assert!(session.text().contains("RESIZED"));
}

#[test]
fn scenario_a_session_is_closed() {
    // Close must stop the shell and join its reader thread. A thread left blocked on a
    // read that never ends would hang this test rather than fail it — which is exactly
    // the failure worth catching, and is what an EOF-only design does under ConPTY.
    let session = Session::start(Size::new(80, 24).unwrap());
    session.close().unwrap();

    let cause = session.wait_for_exit();
    assert_ne!(
        cause.tag(),
        "failed",
        "a deliberate close is a clean end, not a session failure: {cause}"
    );

    // Idempotent: the shell may have exited before the close landed.
    session.close().unwrap();
}

#[test]
fn output_arrives_byte_transparently_including_control_sequences() {
    // The bytes a shell emits must reach the sink unmodified — not escaped, not stripped,
    // not re-encoded (spec `terminal-session`: "Session input and output are
    // byte-transparent").
    let session = Session::start(Size::new(80, 24).unwrap());
    session.run_then_exit(b"echo MARKER_ONE");

    let raw = session.raw();
    assert!(
        raw.windows(10).any(|w| w == b"MARKER_ONE"),
        "the marker survived the round trip"
    );
    // Every shell under a PTY emits control sequences unprompted; if any layer were
    // escaping or filtering them, none would be present as raw bytes.
    assert!(
        raw.contains(&0x1b),
        "escape bytes reached the sink unmodified rather than being stripped"
    );
}

#[test]
fn scenario_input_after_the_session_ended() {
    // Writing to a closed session must be refused with a reason, not panic and not
    // silently succeed into a dead handle.
    let session = Session::start(Size::new(80, 24).unwrap());
    session.close().unwrap();
    match session.write(b"echo too late\r\n") {
        Err(SessionError::Io(_)) => {}
        Ok(()) => panic!("a closed session must not accept input"),
        Err(other) => panic!("expected an i/o refusal, got {other:?}"),
    }
}
