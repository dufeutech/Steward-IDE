//! Making the sessions this process starts interruptible (design D2b).
//!
//! The whole of the Windows answer to "interrupt what the session is running", and
//! deliberately the only place in this repository that calls the Windows API directly. It
//! runs once, before the first session is spawned, and nothing here knows what a session is.
//!
//! The delivery itself is not here, because it does not need to be: the interrupt character
//! written to the pseudoconsole is what `conhost` turns into a control event, on Windows as
//! on Unix. What this file fixes is the reason that stopped working.

use windows_sys::Win32::System::Console::{SetConsoleCtrlHandler, CTRL_C_EVENT};

/// Make every session this process starts interruptible, before it starts one.
///
/// `CREATE_NEW_PROCESS_GROUP` gives a process the "ignore Ctrl+C" attribute, and that
/// attribute is **inherited by every child**. A launcher that uses the flag — which is how a
/// development runner keeps Ctrl+C in its own terminal from killing what it started — hands
/// the attribute to this application, and this application hands it to the `conhost` that
/// `CreatePseudoConsole` spawns, to the shell, and to everything the shell runs. Nothing on
/// that pseudoconsole can then receive a control event, whether `conhost` synthesises one
/// from a byte or another process raises one with `GenerateConsoleCtrlEvent`. That is the
/// defect this change existed to fix, and it was never in the terminal at all.
///
/// `SetConsoleCtrlHandler(NULL, FALSE)` clears the attribute. Inheritance is fixed at
/// creation, so this must run **before** the shell is spawned. Microsoft's `node-pty` — the
/// pseudoconsole layer under VS Code's terminal — makes exactly this call in
/// `PtyStartProcess`, immediately after `CreatePseudoConsole` succeeds. `portable-pty` does
/// not, which is why it has to be done here.
///
/// Process-global by nature, and idempotent, so it runs once.
pub fn enable_interrupts_for_sessions() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: FFI into the console API, mutating this process's own control-handling
        // state and nothing else. The order is the invariant: guard first, then clear.
        unsafe {
            // Guard this process *first*. Clearing the attribute below re-arms the default
            // handler, which calls `ExitProcess` — so between these two calls a Ctrl+C
            // pressed in the terminal that launched a development build would kill the
            // application. A handler routine is not inherited by children, where the
            // attribute is, so the shells started afterwards keep the default terminating
            // behaviour that is what makes an interrupt an interrupt.
            SetConsoleCtrlHandler(Some(ignore_ctrl_c_in_this_process), 1);
            SetConsoleCtrlHandler(None, 0);
        }
    });
}

/// Returning `TRUE` means "handled — do not fall through", and the handler this falls
/// through to is the default one, which calls `ExitProcess`.
///
/// # Safety
/// Called by the system on a thread it creates in this process.
unsafe extern "system" fn ignore_ctrl_c_in_this_process(event: u32) -> windows_sys::core::BOOL {
    // Only the interrupt. Close, log-off and shutdown must still fall through, or the
    // application would refuse to exit when the system tells it to.
    if event == CTRL_C_EVENT {
        1
    } else {
        0
    }
}
