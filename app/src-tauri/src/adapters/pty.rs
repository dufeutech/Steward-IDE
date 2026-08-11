//! The OS-facing side of a terminal session (ADR "Pseudo-terminal and child-process
//! control"; design D4).
//!
//! Thin by design: allocate, spawn, shovel bytes, reap. Every rule about *which* session
//! may be addressed and *what* a size may be lives in `core::terminal`. No `portable_pty`
//! type crosses back into the core, so replacing the crate touches this file alone.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::core::terminal::{ExitCause, Pty, PtySpawner, SessionError, Size, SpawnRequest};

/// Bytes handed to the output sink per read. A PTY read returns whatever is available, so
/// this is a ceiling rather than a quantum — it only bounds how much sits in one buffer.
const READ_CHUNK: usize = 8 * 1024;

/// The terminal interrupt character, `ETX`. What a keyboard's interrupt chord puts on the
/// wire, and on Unix all that is needed — the line discipline turns it into a signal, or
/// hands it to a program that asked for raw input (design D4).
const INTERRUPT: u8 = 0x03;

fn pty_size(size: Size) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.columns,
        // The shell is told cells, not pixels. Zero is what "unknown" means here, and is
        // what every terminal that does not report pixel geometry sends.
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Starts sessions on the platform's native PTY — ConPTY on Windows, `openpty` on Unix.
pub struct NativePtySpawner {
    /// Where sessions start. A defined working directory is part of the session contract
    /// (spec `terminal-session`), so it is injected rather than inherited by accident.
    cwd: PathBuf,
}

impl NativePtySpawner {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

impl PtySpawner for NativePtySpawner {
    fn spawn(&self, request: SpawnRequest) -> Result<Box<dyn Pty>, SessionError> {
        // Before the pseudoconsole exists, not after: what this clears is inherited at
        // child-creation time, so a session started first is born uninterruptible for its
        // whole life (design D2b).
        #[cfg(windows)]
        crate::adapters::console_ctrl::enable_interrupts_for_sessions();

        let pair = native_pty_system()
            .openpty(pty_size(request.size))
            .map_err(|e| SessionError::Spawn(format!("could not allocate a terminal: {e}")))?;

        // `CommandBuilder::new` seeds itself from the parent environment, which is what
        // "commands see the user's environment" requires — do not clear it.
        let mut command = CommandBuilder::new(&request.program);
        command.cwd(&self.cwd);

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| SessionError::Spawn(format!("{}: {e}", request.program)))?;

        // Drop the slave now. While this process holds it open, the master's reader never
        // sees EOF, so the reader thread would block forever after the shell exits and the
        // session would never be reported as ended.
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| SessionError::Spawn(format!("could not read the terminal: {e}")))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| SessionError::Spawn(format!("could not write the terminal: {e}")))?;

        // Kill from one thread while another blocks in `wait()`. Without this the waiter
        // below would have to hold a lock the killer also needs, and closing a busy
        // session would deadlock against reaping it.
        let killer = child.clone_killer();

        let SpawnRequest {
            on_output, on_exit, ..
        } = request;
        let finished = Arc::new(AtomicBool::new(false));

        // Two threads, because the two events are genuinely independent.
        //
        // Exit is watched by a thread that blocks in `wait()`. It must NOT be inferred
        // from the reader reaching EOF: under ConPTY the master read side stays open
        // after the shell exits, so an EOF-only design reports no exit at all on Windows
        // — measured, not assumed (see design D3's spike finding).
        std::thread::Builder::new()
            .name("steward-pty-waiter".into())
            .spawn({
                let finished = Arc::clone(&finished);
                move || {
                    let cause = reap(child);
                    finished.store(true, Ordering::SeqCst);
                    on_exit(cause);
                }
            })
            .map_err(|e| SessionError::Spawn(format!("could not start a waiter: {e}")))?;

        // Output is carried by a blocking read loop (design D4, Risks). It ends on EOF,
        // on error, or once the shell has been reaped — whichever comes first.
        let reader_thread = std::thread::Builder::new()
            .name("steward-pty-reader".into())
            .spawn({
                let finished = Arc::clone(&finished);
                move || {
                    let mut reader = reader;
                    let mut buffer = vec![0u8; READ_CHUNK];
                    loop {
                        match reader.read(&mut buffer) {
                            Ok(0) => break,
                            Ok(n) => on_output(&buffer[..n]),
                            // Interrupted is not the end of the stream; anything else is.
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(_) => break,
                        }
                        if finished.load(Ordering::SeqCst) {
                            break;
                        }
                    }
                }
            })
            .map_err(|e| SessionError::Spawn(format!("could not start a reader: {e}")))?;

        Ok(Box::new(NativePty {
            master: Some(pair.master),
            writer: Some(Box::new(writer)),
            killer,
            reader_thread: Some(reader_thread),
        }))
    }
}

/// Wait for the child and classify how it stopped.
///
/// Kept apart from the read loop because the classification — and only the classification
/// — is what the rest of the system sees.
fn reap(mut child: Box<dyn Child + Send + Sync>) -> ExitCause {
    match child.wait() {
        Ok(status) => match status.signal() {
            Some(signal) => ExitCause::Signalled {
                signal: Some(signal.to_string()),
            },
            // `exit_code` is a u32 across platforms; Windows exit codes genuinely use the
            // high bit, so saturate rather than wrap a large value into a negative one.
            None => ExitCause::Exited {
                code: i32::try_from(status.exit_code()).unwrap_or(i32::MAX),
            },
        },
        Err(e) => ExitCause::Failed {
            reason: format!("could not reap the shell: {e}"),
        },
    }
}

struct NativePty {
    /// Dropped on close: releasing the master is what lets a blocked read return, so the
    /// reader thread can end.
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// Taken on close so the thread is joined exactly once (spec: nothing outlives its
    /// session).
    reader_thread: Option<std::thread::JoinHandle<()>>,
}

impl Pty for NativePty {
    fn write(&mut self, bytes: &[u8]) -> Result<(), SessionError> {
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| SessionError::Io("the session is closed".into()))?;
        writer
            .write_all(bytes)
            .and_then(|()| writer.flush())
            .map_err(|e| SessionError::Io(e.to_string()))
    }

    fn resize(&mut self, size: Size) -> Result<(), SessionError> {
        self.master
            .as_ref()
            .ok_or_else(|| SessionError::Io("the session is closed".into()))?
            .resize(pty_size(size))
            .map_err(|e| SessionError::Io(e.to_string()))
    }

    /// Hand the interrupt character to the terminal and let the platform decide what it
    /// means (design D2b, D4).
    ///
    /// One implementation on both platforms, because both make the same distinction in the
    /// same place. Unix's line discipline raises `SIGINT` for the foreground process group
    /// in canonical mode and delivers the byte to a program that asked for raw input;
    /// Windows' `conhost` raises the control event while processed input is on and delivers
    /// the byte once a program has taken raw control. Re-deriving that decision here — with
    /// `tcgetpgrp` and `killpg`, or by attaching to the shell's console — would replace a
    /// correct kernel behaviour with a racy copy of it.
    ///
    /// Nothing is passed in alongside the session for the same reason: the surface has
    /// nothing to observe on the platform's behalf. That observation existed only for a
    /// mechanism that had to choose the delivery form itself (design D3), and that
    /// mechanism is gone.
    fn interrupt(&mut self) -> Result<(), SessionError> {
        if self.writer.is_none() {
            return Err(SessionError::Io("the session is closed".into()));
        }
        self.write(&[INTERRUPT])
    }

    fn close(&mut self) -> Result<(), SessionError> {
        // Killing is best-effort: the shell exiting on its own first is the ordinary
        // case, and a session that is already gone is closed, not broken.
        let _ = self.killer.kill();
        // Release both ends. Dropping the master is what unblocks a read that the kill
        // alone would leave waiting, which is what lets the reader thread finish.
        self.writer = None;
        self.master = None;
        if let Some(thread) = self.reader_thread.take() {
            let _ = thread.join();
        }
        Ok(())
    }
}

impl Drop for NativePty {
    /// Belt and braces for the application-exit path: whatever route takes a session out
    /// of the registry, the shell behind it stops (spec scenario "The application exits
    /// with sessions open").
    fn drop(&mut self) {
        if self.reader_thread.is_some() {
            let _ = self.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_is_reported_not_panicked() {
        // Spec scenario "The shell cannot be started": a reason the surface can show,
        // with the application still usable.
        let spawner = NativePtySpawner::new(std::env::temp_dir());
        let outcome = spawner.spawn(SpawnRequest {
            program: "steward-no-such-shell-exists".into(),
            size: Size::new(80, 24).unwrap(),
            on_output: Box::new(|_| {}),
            on_exit: Box::new(|_| {}),
        });
        // `Box<dyn Pty>` is deliberately not `Debug` — the handle is opaque — so unwrap
        // the error by hand rather than widening the port for a test's convenience.
        let Err(err) = outcome else {
            panic!("a program that does not exist must not start a session");
        };
        match err {
            SessionError::Spawn(reason) => {
                assert!(
                    reason.contains("steward-no-such-shell-exists"),
                    "the reason names what could not be started: {reason}"
                );
            }
            other => panic!("expected a spawn failure, got {other:?}"),
        }
    }

    #[test]
    fn size_conversion_keeps_columns_and_rows_the_right_way_round() {
        // Transposing these is silent and disastrous: the shell lays out to the wrong
        // axis and every full-screen program renders wrong.
        let converted = pty_size(Size::new(120, 40).unwrap());
        assert_eq!(converted.cols, 120);
        assert_eq!(converted.rows, 40);
    }
}
