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

    fn interrupt(&self) -> Result<(), SessionError> {
        self.pty
            .lock()
            .expect("poisoned")
            .as_mut()
            .expect("session is live")
            .interrupt()
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
        // Two consecutive unchanged samples, not one. A command that has been echoed but has
        // not yet produced output looks identical to a settled shell, and one 250 ms sample
        // is short enough to land in that gap — `mode con` under load did, about half the
        // time, and the session was told to `exit` before its output arrived.
        const UNCHANGED_SAMPLES_MEANING_SETTLED: u8 = 2;

        let deadline = std::time::Instant::now() + LIMIT;
        let mut last = usize::MAX;
        let mut unchanged = 0;
        while std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(250));
            let seen = self.raw().len();
            if seen > 0 && seen == last {
                unchanged += 1;
                if unchanged >= UNCHANGED_SAMPLES_MEANING_SETTLED {
                    return;
                }
            } else {
                unchanged = 0;
            }
            last = seen;
        }
        panic!("the shell never settled; got:\n{}", self.text());
    }

    /// How long until `needle` appears, or `None` if it never does.
    fn wait_for(&self, needle: &str, within: Duration) -> Option<Duration> {
        let started = std::time::Instant::now();
        while started.elapsed() < within {
            if self.text().contains(needle) {
                return Some(started.elapsed());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    /// Start something that runs for ~21 seconds, and confirm it is actually running
    /// before anything is measured against it.
    ///
    /// Liveness is watched as output growth rather than matched as a phrase: `ping`'s
    /// wording is localised, and a check that only holds on English Windows would be
    /// measuring the wrong thing.
    fn start_a_long_command(&self, command: &[u8]) {
        self.wait_until_quiet();
        let before = self.raw().len();
        self.write(command).unwrap();
        self.write(b"\r\n").unwrap();
        std::thread::sleep(Duration::from_secs(3));
        assert!(
            self.raw().len() > before,
            "the long command must be running before the interrupt is measured; got:\n{}",
            self.text()
        );
    }

    /// Ask the shell to echo a marker, and time how long it takes to answer.
    ///
    /// The marker is written so that the *typed* line differs from the *printed* one —
    /// `^_` under `cmd`, `''` under `sh`. Without that the marker would appear as soon as
    /// the terminal echoed the keystrokes, and the test would pass whether or not the
    /// shell ever regained control.
    fn time_until_the_shell_answers(&self, within: Duration) -> Option<Duration> {
        let command: &[u8] = if cfg!(windows) {
            b"echo STEWARD_INTERRUPT^_OK\r\n"
        } else {
            b"echo STEWARD_INTERRUPT''_OK\r\n"
        };
        self.write(command).unwrap();
        self.wait_for("STEWARD_INTERRUPT_OK", within)
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

/// Something that keeps running long enough that "it stopped" and "it finished" can never
/// be confused: ~21 seconds against a two-second budget.
fn a_long_command() -> &'static [u8] {
    if cfg!(windows) {
        b"ping -n 25 127.0.0.1"
    } else {
        b"ping -c 25 127.0.0.1"
    }
}

/// The budget separating a working interrupt from a command that ran to completion. Every
/// candidate refuted in `terminal-surface` design D4c landed at ~21s.
const INTERRUPT_BUDGET: Duration = Duration::from_secs(2);

#[test]
fn scenario_a_running_command_is_interrupted() {
    // The defect this change exists to fix, through the real adapter rather than the spike:
    // the command stops, the session does not, and it executes input afterwards.
    let session = Session::start(Size::new(80, 24).unwrap());
    session.start_a_long_command(a_long_command());

    session.interrupt().unwrap();

    let answered = session
        .time_until_the_shell_answers(LIMIT)
        .unwrap_or_else(|| panic!("the shell never came back; got:\n{}", session.text()));
    assert!(
        answered < INTERRUPT_BUDGET,
        "the running command must stop rather than run to completion — the shell took \
         {answered:?}, and ~21s is the signature of the command finishing on its own"
    );
}

#[test]
fn scenario_the_interrupt_reaches_what_the_command_started() {
    // One level deeper: the shell runs a child which runs the long command. If only the
    // immediate child were signalled, the shell would still be waiting on the grandchild
    // and could not answer inside the budget.
    let session = Session::start(Size::new(80, 24).unwrap());
    let nested: &[u8] = if cfg!(windows) {
        b"cmd /c ping -n 25 127.0.0.1"
    } else {
        b"sh -c 'ping -c 25 127.0.0.1'"
    };
    session.start_a_long_command(nested);

    session.interrupt().unwrap();

    let answered = session
        .time_until_the_shell_answers(LIMIT)
        .unwrap_or_else(|| panic!("the shell never came back; got:\n{}", session.text()));
    assert!(
        answered < INTERRUPT_BUDGET,
        "nothing from the interrupted command may be left running — the shell took {answered:?}"
    );
}

#[test]
fn a_session_started_after_an_interrupt_is_still_interruptible() {
    // The inheritance hazard, measured. `SetConsoleCtrlHandler(NULL, TRUE)` — the form of
    // the guard this change started with — is inherited by child processes, so a shell
    // spawned once an interrupt had happened would have been born unable to be interrupted.
    // A handler routine is not inherited (design D2), and this is what says so.
    let first = Session::start(Size::new(80, 24).unwrap());
    first.start_a_long_command(a_long_command());
    first.interrupt().unwrap();

    let second = Session::start(Size::new(80, 24).unwrap());
    second.start_a_long_command(a_long_command());
    second.interrupt().unwrap();

    let answered = second
        .time_until_the_shell_answers(LIMIT)
        .unwrap_or_else(|| panic!("the second shell never came back; got:\n{}", second.text()));
    assert!(
        answered < INTERRUPT_BUDGET,
        "a session started after an interrupt must still be interruptible; took {answered:?}"
    );
}

#[test]
fn scenario_an_idle_session_is_interrupted() {
    // Harmless, and the session goes on accepting input.
    let session = Session::start(Size::new(80, 24).unwrap());
    session.wait_until_quiet();

    session.interrupt().unwrap();

    assert!(
        session
            .time_until_the_shell_answers(LIMIT)
            .is_some_and(|t| t < INTERRUPT_BUDGET),
        "an interrupt at a prompt changes nothing; got:\n{}",
        session.text()
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
