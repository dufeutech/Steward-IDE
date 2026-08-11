//! Raising `CTRL_C_EVENT` on another process's console (design D2; ADRs 1 and 3).
//!
//! This is the whole of the Windows answer to "interrupt what the session is running", and
//! it is deliberately the only place in this repository that calls the Windows API
//! directly. Nothing here knows what a session is — it takes a process identifier and
//! raises an event on that process's console.
//!
//! **Every step below is measured.** The obvious readings of the platform documentation
//! produce a sequence that kills this process with `STATUS_CONTROL_C_EXIT` on its first
//! use; three of them do. The spike that established which sequence survives is
//! `tests/terminal_interrupt_windows.rs`, and the failing shapes are recorded in the
//! change's design under D2, "What the spike changed". Do not reorder this without
//! re-running it.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use windows_sys::Win32::System::Console::{
    AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, GetConsoleWindow, SetConsoleCtrlHandler,
    ATTACH_PARENT_PROCESS, CTRL_C_EVENT,
};

use crate::core::terminal::SessionError;

/// How long to wait for our own event before giving up and detaching anyway.
///
/// Reached only if the event is never delivered to us, which has not been observed. The
/// cost of the bound being hit is one slow keypress; the cost of not having it is a
/// permanently held console lock.
const DELIVERY_BOUND: Duration = Duration::from_millis(500);

/// Set by the handler when the event we raised comes back to us.
static DELIVERED: AtomicBool = AtomicBool::new(false);

/// A process may be attached to exactly one console, so the attach window is a critical
/// section for the whole process — not per session.
fn console_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    // Poisoning would mean a previous interrupt panicked mid-sequence. The console is
    // restored on every path below, so the next interrupt is no less safe than the first.
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The argument that turns the helper binary into an interrupt rather than an application.
pub const HELPER_ARG: &str = "--steward-raise-interrupt";

/// Interrupt everything running on `pid`'s console, by asking `helper` to do it.
///
/// **The application never attaches to a console itself, and that is the whole point**
/// (design D2a). Raised from inside this process the event is delivered to nobody — not the
/// shell, not even this process, whose handler is registered and fires when the identical
/// sequence runs from a test binary. A helper is born clean: no console, no handler history,
/// nothing to restore, and no way for a mistake here to take the application down with it.
pub fn interrupt(pid: u32, helper: &Path) -> Result<(), SessionError> {
    let status = Command::new(helper)
        .arg(HELPER_ARG)
        .arg(pid.to_string())
        // Without this a console-subsystem build flashes a console window on every
        // keypress. The helper needs no console of its own — it borrows the shell's.
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| {
            SessionError::Io(format!(
                "could not run {} to interrupt the session: {e}",
                helper.display()
            ))
        })?;

    match status.status.code() {
        Some(0) => Ok(()),
        // The helper says why on stderr; carrying it through is what keeps a failed
        // interrupt diagnosable from the surface rather than a bare exit code.
        Some(code) => Err(SessionError::Io(format!(
            "the interrupt was not delivered (helper exited with {code}): {}",
            String::from_utf8_lossy(&status.stderr).trim()
        ))),
        None => Err(SessionError::Io(
            "the interrupt helper was terminated before it could report".into(),
        )),
    }
}

/// Raise the control event **in this process**, which must be the helper.
///
/// Called only from the helper binary. Everything it mutates — console attachment, the
/// control-handler list — dies with the process moments later, which is why this is the
/// only place the sequence is allowed to run.
pub fn raise_here(pid: u32) -> Result<(), SessionError> {
    let _guard = console_lock();
    // SAFETY: every call below is FFI into the console API. The invariant the unsafe block
    // maintains is that this process leaves the function attached to whatever console it
    // entered with — every early return goes through `restore`.
    unsafe {
        let had_console = !GetConsoleWindow().is_null();

        // A process can hold only one console, so ours has to go before theirs arrives.
        FreeConsole();

        if AttachConsole(pid) == 0 {
            let err = std::io::Error::last_os_error();
            restore(had_console);
            // The ordinary cause is a shell that exited between the lookup and here.
            return Err(SessionError::Io(format!(
                "could not reach the shell's console to interrupt it: {err}"
            )));
        }

        // Survive our own event. This must happen *after* the attach: `FreeConsole` drops
        // the process's handler list, so a handler registered before the switch is gone by
        // the time it is needed. Because the list starts empty on every attach, exactly one
        // registration exists at a time and there is nothing to clean up.
        if SetConsoleCtrlHandler(Some(survive_our_own_event), 1) == 0 {
            let err = std::io::Error::last_os_error();
            restore(had_console);
            return Err(SessionError::Io(format!(
                "could not guard against our own interrupt, so it was not raised: {err}"
            )));
        }

        DELIVERED.store(false, Ordering::SeqCst);
        let raised = GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0);
        let err = std::io::Error::last_os_error();

        // Stay attached until the event has reached us. Detaching first would unregister
        // the handler above and hand our own event to the default one, which calls
        // `ExitProcess`.
        await_delivery();
        restore(had_console);

        if raised == 0 {
            return Err(SessionError::Io(format!(
                "the interrupt could not be raised: {err}"
            )));
        }
    }
    Ok(())
}

/// # Safety
/// FFI. Called on every exit path of `interrupt`, including the failing ones.
unsafe fn restore(had_console: bool) {
    FreeConsole();
    if had_console {
        // Without this a development run detaches from the terminal it was launched from
        // and everything it prints afterwards goes nowhere.
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// # Safety
/// FFI. Must be called while still attached to the console that was raised at.
unsafe fn await_delivery() {
    let deadline = Instant::now() + DELIVERY_BOUND;
    while !DELIVERED.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Returning `TRUE` means "handled — do not fall through", and the handler this falls
/// through to is the default one, which calls `ExitProcess`.
///
/// A handler routine rather than `SetConsoleCtrlHandler(NULL, TRUE)`'s ignore attribute,
/// for two measured reasons: the attribute has to be cleared afterwards, and clearing it
/// races the asynchronous delivery of the event it was protecting against; and the
/// attribute is *inherited by child processes* where a handler routine is not. Inheriting
/// it would mean any shell started during the window was born unable to be interrupted —
/// the exact opposite of the point.
///
/// # Safety
/// Called by the system on a thread it creates in this process.
unsafe extern "system" fn survive_our_own_event(event: u32) -> windows_sys::core::BOOL {
    // Only what we raise. Close, log-off and shutdown must still fall through, or the
    // application would refuse to exit when the system tells it to.
    if event == CTRL_C_EVENT {
        DELIVERED.store(true, Ordering::SeqCst);
        1
    } else {
        0
    }
}
