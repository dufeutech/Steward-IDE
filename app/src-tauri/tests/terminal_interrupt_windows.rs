#![cfg(windows)]
//! The measurement that design D2 rests on, taken before any of it is built.
//!
//! `terminal-surface` design D4c records four candidates for why the interrupt chord never
//! becomes a control event for a running command, each refuted by measurement. This file
//! tests the fifth and last one: that the fix is not in the byte stream at all — that a
//! terminal *raises* the control event on the shell's console rather than hoping the
//! console host synthesises one.
//!
//! Deliberately written against `portable_pty` directly rather than through
//! `adapters::pty`. A spike that fails must refute the platform hypothesis, not our
//! wiring, and there is no wiring yet to blame.
//!
//! **Everything here touches process-global state.** A process may be attached to exactly
//! one console, so every section that attaches takes `console_lock()` — cargo runs these
//! tests on parallel threads of one process, and without the lock they would corrupt each
//! other's console rather than measure anything. That lock is the prototype of the one
//! design D2 requires in the adapter.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, GetConsoleMode, GetConsoleWindow,
    SetConsoleCtrlHandler, ATTACH_PARENT_PROCESS, CTRL_C_EVENT, ENABLE_PROCESSED_INPUT,
};

/// Diagnostics that survive losing the console.
///
/// `FreeConsole` invalidates the inherited stdout/stderr handles, so anything printed
/// during the sequence can vanish — including the panic message of a test that died
/// mid-sequence. A file is the only witness that outlives every step here.
fn trace(message: &str) {
    use std::fs::OpenOptions;
    let path = std::env::temp_dir().join("steward-interrupt-spike.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{message}");
        let _ = f.flush();
    }
}

/// ConPTY opens by asking the terminal where the cursor is and blocks until something
/// answers. No emulator here, so the harness answers — without this a Windows shell never
/// reaches its prompt and every test below times out looking like a PTY bug
/// (`terminal-surface` design D4b).
const CURSOR_QUERY: &[u8] = b"\x1b[6n";
const CURSOR_ANSWER: &[u8] = b"\x1b[1;1R";

/// The measurement D4c's probes were taken against: `ping -n 25` runs for ~21 seconds, so
/// "the shell answered quickly" and "the command ran to completion" are never ambiguous.
const INTERRUPT_BUDGET: Duration = Duration::from_secs(2);
const RUN_TO_COMPLETION: Duration = Duration::from_secs(25);

/// Console attachment is per-process, not per-session.
fn console_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    // A poisoned lock here means another test panicked mid-sequence. The console state is
    // restored on every path below, so continuing is better than cascading failures.
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Design D2's sequence, by hand.
///
/// # Safety
/// Every call is an FFI call into the console API. The invariant that matters is that the
/// process leaves this function attached to whatever console it started with, and with its
/// Ctrl+C handling as it found it — every early return goes through `restore`.
unsafe fn raise_interrupt(pid: u32) -> Result<(), String> {
    let _guard = console_lock();

    // 1. Remember whether we have a console of our own to come back to.
    let had_console = !GetConsoleWindow().is_null();

    // 2. A process may be attached to only one console at a time.
    FreeConsole();

    // 3. Join the pseudoconsole the session's shell is running on.
    if AttachConsole(pid) == 0 {
        let err = std::io::Error::last_os_error();
        restore_console(had_console);
        return Err(format!("AttachConsole({pid}) failed: {err}"));
    }

    // 3a. Survive our own event — **on the console we have just joined**.
    //
    //     This placement is measured, not chosen. Registering the handler before the
    //     console switch does not survive it: `FreeConsole` drops the process's handler
    //     list, and a raise afterwards kills this process with `STATUS_CONTROL_C_EXIT`.
    //     Because the list starts empty on each attach, exactly one entry accumulates per
    //     interrupt — there is nothing to remove and no leak.
    if SetConsoleCtrlHandler(Some(swallow_ctrl_c), 1) == 0 {
        let err = std::io::Error::last_os_error();
        restore_console(had_console);
        return Err(format!("the control handler would not install: {err}"));
    }

    // 4. There is deliberately no console-mode probe here. Asking the console what the
    //    running program wants was the original design D3, and it does not work — see
    //    `processed_input_does_not_distinguish_a_prompt_from_a_raw_mode_program` below.
    //    The decision now comes from the emulator, above this layer, so this sequence has
    //    only one job.
    //
    // 5. (The guard went in at step 3a.)
    //
    // 6. `0` means every process sharing this console — the session's shell and its
    //    descendants. It is also the only value that works: the reference states CTRL_C
    //    cannot be limited to a process group, and a nonzero group id succeeds while
    //    delivering nothing.
    DELIVERED.store(false, Ordering::SeqCst);
    let raised = GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0);
    let raise_err = std::io::Error::last_os_error();

    // 6a. Stay attached until our own handler has seen the event. Detaching first drops the
    //     handler list and the event lands on the default handler instead — which is
    //     `ExitProcess`.
    await_our_own_event();

    // 7. Leave the session's console and come back to our own.
    restore_console(had_console);

    if raised == 0 {
        return Err(format!("GenerateConsoleCtrlEvent failed: {raise_err}"));
    }
    Ok(())
}

/// Swallow the control event this process raises, so raising one does not kill us.
///
/// **Two measurements shaped this, and both contradict design D2 step 5 as written.**
///
/// First, the documented `SetConsoleCtrlHandler(NULL, TRUE)` ignore attribute — set before
/// the raise, cleared after the detach — kills this process every time; one raise is
/// enough, and the crash is `STATUS_CONTROL_C_EXIT` (0xc000013a). The reason is in the
/// platform's own description of delivery: the system creates *a new thread in each
/// attached process* to run the handlers, so delivery is asynchronous, and clearing the
/// attribute at step 7 re-arms the default handler — which calls `ExitProcess` — before our
/// own event has arrived.
///
/// Second, a real handler routine fixes that only if it is registered on the console being
/// raised at. Installed once before the console switch it also dies: `FreeConsole` drops
/// the process's handler list. Registered after `AttachConsole` it holds.
///
/// A handler routine is the better guard for a second reason too: handler routines are
/// *not* inherited by child processes, where the ignore attribute is. The session's shell
/// and its children therefore keep responding to interrupts normally, which is the entire
/// point of raising one, and the inheritance hazard in design D2's Risks disappears rather
/// than needing a lock to contain it.
///
/// Returning `TRUE` means "handled — do not fall through to the default handler", and the
/// default handler is the one that calls `ExitProcess`.
///
/// # Safety
/// Called by the system on a thread it creates in this process.
unsafe extern "system" fn swallow_ctrl_c(event: u32) -> windows_sys::core::BOOL {
    // Only the event we raise. Everything else — close, log-off, shutdown — must fall
    // through to the default so the process still exits when the system says so.
    if event == CTRL_C_EVENT {
        DELIVERED.store(true, Ordering::SeqCst);
        1
    } else {
        0
    }
}

/// Set by the handler above when our own event arrives.
///
/// The guard only protects while it is registered, and `FreeConsole` unregisters it — so
/// detaching immediately after the raise is what killed this process even with a handler
/// installed. Waiting for our own delivery before detaching closes that window without
/// guessing at a sleep: the common case returns in single-digit milliseconds, and the
/// bound below is what stops a lost event hanging a keypress forever.
static DELIVERED: AtomicBool = AtomicBool::new(false);
const DELIVERY_BOUND: Duration = Duration::from_millis(500);

/// # Safety
/// FFI. Must be called while still attached to the console the event was raised on.
unsafe fn await_our_own_event() {
    let deadline = Instant::now() + DELIVERY_BOUND;
    while !DELIVERED.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// # Safety
/// FFI. Called on every exit path of the sequence above, including the failing ones.
unsafe fn restore_console(had_console: bool) {
    FreeConsole();
    if had_console {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// The attached console's input mode, or `None` if it cannot be read.
///
/// # Safety
/// FFI, and only meaningful while attached to the console being asked about.
unsafe fn console_input_mode() -> Option<u32> {
    // `CONIN$` reaches the *attached* console's input buffer, which is the point: the
    // process's own stdin handle would answer for the wrong console.
    const CONIN: [u16; 7] = [
        b'C' as u16,
        b'O' as u16,
        b'N' as u16,
        b'I' as u16,
        b'N' as u16,
        b'$' as u16,
        0,
    ];
    let handle = CreateFileW(
        CONIN.as_ptr(),
        GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        std::ptr::null(),
        OPEN_EXISTING,
        0,
        std::ptr::null_mut(),
    );
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut mode = 0u32;
    let read = GetConsoleMode(handle, &mut mode);
    CloseHandle(handle);
    (read != 0).then_some(mode)
}

/// Read the input mode of another process's console without raising anything.
///
/// # Safety
/// FFI. Attaches and detaches under the same lock as the sequence above.
unsafe fn probe_input_mode(pid: u32) -> Result<u32, String> {
    let _guard = console_lock();
    let had_console = !GetConsoleWindow().is_null();
    FreeConsole();
    if AttachConsole(pid) == 0 {
        let err = std::io::Error::last_os_error();
        restore_console(had_console);
        return Err(format!("AttachConsole({pid}) failed: {err}"));
    }
    let mode = console_input_mode();
    restore_console(had_console);
    mode.ok_or_else(|| "the attached console would not report its input mode".to_string())
}

/// A real shell on a real pseudoconsole, plus enough to watch what it says.
struct Spike {
    pid: u32,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    output: Arc<Mutex<Vec<u8>>>,
    child: Box<dyn Child + Send + Sync>,
    _master: Box<dyn MasterPty + Send>,
}

impl Spike {
    fn start() -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("a pseudoconsole can be allocated on this machine");

        let mut command = CommandBuilder::new("cmd.exe");
        command.cwd(std::env::temp_dir());
        let child = pair
            .slave
            .spawn_command(command)
            .expect("cmd.exe exists on every Windows machine");
        // Holding the slave open would keep the master's reader from ever seeing EOF.
        drop(pair.slave);

        let pid = child
            .process_id()
            .expect("a ConPTY child has a process identifier");

        let mut reader = pair
            .master
            .try_clone_reader()
            .expect("the master can be read");
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(
            pair.master
                .take_writer()
                .expect("the master can be written"),
        ));
        let output: Arc<Mutex<Vec<u8>>> = Arc::default();

        std::thread::spawn({
            let output = Arc::clone(&output);
            let answering = Arc::clone(&writer);
            move || {
                let mut buffer = vec![0u8; 8 * 1024];
                while let Ok(n) = reader.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    let chunk = &buffer[..n];
                    output.lock().expect("poisoned").extend_from_slice(chunk);
                    if chunk.windows(CURSOR_QUERY.len()).any(|w| w == CURSOR_QUERY) {
                        let mut w = answering.lock().expect("poisoned");
                        let _ = w.write_all(CURSOR_ANSWER);
                        let _ = w.flush();
                    }
                }
            }
        });

        Self {
            pid,
            writer,
            output,
            child,
            _master: pair.master,
        }
    }

    fn write(&self, bytes: &[u8]) {
        let mut w = self.writer.lock().expect("poisoned");
        w.write_all(bytes).expect("the session accepts input");
        w.flush().expect("the session accepts input");
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().expect("poisoned")).into_owned()
    }

    fn len(&self) -> usize {
        self.output.lock().expect("poisoned").len()
    }

    /// Block until the shell stops producing output — the only available proxy for "ready",
    /// since a prompt is just bytes and differs per shell.
    fn wait_until_quiet(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut last = usize::MAX;
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(250));
            let seen = self.len();
            if seen > 0 && seen == last {
                return;
            }
            last = seen;
        }
        panic!("the shell never settled; got:\n{}", self.text());
    }

    /// How long until `needle` appears, or `None` if it never does.
    fn wait_for(&self, needle: &str, within: Duration) -> Option<Duration> {
        let started = Instant::now();
        while started.elapsed() < within {
            if self.text().contains(needle) {
                return Some(started.elapsed());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    /// Start a command that runs for ~21 seconds and confirm it is actually running before
    /// anything is measured against it.
    ///
    /// Liveness is checked by watching the output grow rather than by matching a phrase:
    /// `ping`'s wording is localised, and a test that passes only on English Windows would
    /// be measuring the wrong thing.
    fn start_a_long_command(&self) {
        self.wait_until_quiet();
        let before = self.len();
        self.write(b"ping -n 25 127.0.0.1\r\n");
        std::thread::sleep(Duration::from_secs(3));
        assert!(
            self.len() > before,
            "the long command must be running before the interrupt is measured; got:\n{}",
            self.text()
        );
    }
}

impl Drop for Spike {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[ignore = "diagnostic: run explicitly when the sequence kills its own process"]
fn spike_diagnose_where_the_sequence_dies() {
    // Not a property — a witness. Every step of design D2 is traced to a file so that a
    // process which dies mid-sequence still says where.
    trace("--- run ---");
    let spike = Spike::start();
    spike.start_a_long_command();
    trace(&format!("shell pid {}, long command running", spike.pid));

    unsafe {
        let had_console = !GetConsoleWindow().is_null();
        trace(&format!("had_console={had_console}"));

        trace(&format!("FreeConsole -> {}", FreeConsole()));
        let attached = AttachConsole(spike.pid);
        trace(&format!(
            "AttachConsole({}) -> {attached} ({})",
            spike.pid,
            std::io::Error::last_os_error()
        ));
        if attached == 0 {
            restore_console(had_console);
            trace("attach failed; restored");
            return;
        }

        trace(&format!("input mode = {:?}", console_input_mode()));

        // Re-install while attached to *this* console, in case the handler list does not
        // survive a console switch.
        let reinstalled = SetConsoleCtrlHandler(Some(swallow_ctrl_c), 1);
        trace(&format!("re-install handler -> {reinstalled}"));

        trace("about to raise");
        let raised = GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0);
        trace(&format!(
            "raised -> {raised} ({})",
            std::io::Error::last_os_error()
        ));

        std::thread::sleep(Duration::from_millis(500));
        trace("survived 500ms after the raise");

        restore_console(had_console);
        trace("console restored");
    }

    spike.write(b"echo DIAG^_OK\r\n");
    let answered = spike.wait_for("DIAG_OK", RUN_TO_COMPLETION);
    trace(&format!("shell answered after {answered:?}"));
    trace("--- end ---");
}

#[test]
fn spike_the_control_event_interrupts_a_running_command() {
    // Task 2.1. This is the whole hypothesis: if the shell answers in under two seconds the
    // control event reached the child; ~21 seconds means `ping` ran to completion and design
    // D2 is refuted exactly as its four predecessors were.
    let spike = Spike::start();
    spike.start_a_long_command();

    let at = Instant::now();
    unsafe { raise_interrupt(spike.pid) }.expect("the sequence completes");

    // The caret is load-bearing: `cmd` echoes the line as typed, so a plain marker would
    // appear in the output whether or not the command ever ran. `^_` is typed as `^_` and
    // printed as `_`, so only the *executed* echo can match.
    spike.write(b"echo STEWARD_INTERRUPT^_OK\r\n");
    let answered = spike
        .wait_for("STEWARD_INTERRUPT_OK", RUN_TO_COMPLETION)
        .unwrap_or_else(|| panic!("the shell never came back at all; got:\n{}", spike.text()));

    let elapsed = at.elapsed();
    println!("interrupt → shell answered in {elapsed:?} (marker at {answered:?})");
    assert!(
        elapsed < INTERRUPT_BUDGET,
        "the running command must stop, not run to completion — the shell took {elapsed:?}, \
         which is the ~21s signature of `ping` finishing on its own; got:\n{}",
        spike.text()
    );
}

#[test]
fn spike_the_session_survives_being_interrupted() {
    // The other half of the spec scenario: interrupting stops the command, not the session.
    let spike = Spike::start();
    spike.start_a_long_command();
    unsafe { raise_interrupt(spike.pid) }.expect("the sequence completes");

    spike.write(b"echo STILL^_ALIVE\r\n");
    assert!(
        spike.wait_for("STILL_ALIVE", INTERRUPT_BUDGET).is_some(),
        "the shell must still execute input after an interrupt; got:\n{}",
        spike.text()
    );
}

#[test]
fn spike_raising_the_event_a_hundred_times_does_not_kill_this_process() {
    // Task 2.2. `SetConsoleCtrlHandler(None, TRUE)` is what stops the application dying
    // along with the command, and the platform delivers the event asynchronously — so the
    // ordering in `raise_interrupt` is measured here rather than assumed. Reaching the end
    // of this test *is* the assertion: if the guard were insufficient or mis-ordered, the
    // test binary would be terminated instead of failing.
    let spike = Spike::start();
    spike.wait_until_quiet();

    for i in 0..100 {
        unsafe { raise_interrupt(spike.pid) }
            .unwrap_or_else(|e| panic!("the sequence completes on iteration {i}: {e}"));
    }

    spike.write(b"echo SURVIVED^_ALL\r\n");
    assert!(
        spike
            .wait_for("SURVIVED_ALL", Duration::from_secs(10))
            .is_some(),
        "the session survives a hundred interrupts too; got:\n{}",
        spike.text()
    );
}

#[test]
fn spike_the_console_is_restored_after_the_sequence() {
    // Task 2.3. `FreeConsole` detaches a process that *was* launched from a terminal, after
    // which its own output goes nowhere. Invisible in the packaged application (windowed,
    // no console of its own) and very visible under `cargo run`, which is where this
    // behaviour gets developed.
    let had_console = unsafe { !GetConsoleWindow().is_null() };

    let spike = Spike::start();
    spike.wait_until_quiet();
    unsafe { raise_interrupt(spike.pid) }.expect("the sequence completes");

    assert_eq!(
        unsafe { !GetConsoleWindow().is_null() },
        had_console,
        "the sequence must leave this process attached to whatever console it started with"
    );

    // Meaningful only under `--nocapture`, where stdout really is the console handle;
    // under captured output it is a pipe and survives regardless. Asserted either way so
    // the stricter run is the one that reports.
    let mut out = std::io::stdout();
    writeln!(
        out,
        "console restored; stdout still reaches its destination"
    )
    .expect("stdout still accepts writes after the sequence");
    out.flush()
        .expect("stdout still flushes after the sequence");
}

#[test]
fn processed_input_does_not_distinguish_a_prompt_from_a_raw_mode_program() {
    // Task 2.4, and the reason design D3 asks the emulator instead of the console.
    //
    // The question was whether `ENABLE_PROCESSED_INPUT` on a *pseudo*console tracks what
    // the running program set, the way it does on a real one. It does not — and this test
    // asserts that finding rather than the expectation it replaced, so that a future
    // Windows or ConPTY release which *does* propagate the flag fails here loudly instead
    // of leaving D3 taking the long way round for no reason.
    let spike = Spike::start();
    spike.wait_until_quiet();

    let at_prompt = unsafe { probe_input_mode(spike.pid) }.expect("the console reports its mode");
    println!("input mode at a shell prompt: {at_prompt:#06x}");
    assert_ne!(
        at_prompt & ENABLE_PROCESSED_INPUT,
        0,
        "a shell at a prompt leaves processed input on"
    );

    // `TreatControlCAsInput` is .NET's name for exactly this flag, and PowerShell is on
    // every Windows machine — unlike `vim`, which is what the by-hand check uses.
    spike.write(
        b"powershell -NoProfile -Command \"[Console]::TreatControlCAsInput=$true; Write-Host RAWMODE_READY; [Console]::ReadKey($true) | Out-Null\"\r\n",
    );

    // Polled rather than slept: "the flag never changed" and "PowerShell had not started
    // yet" are different findings, and a single sleep cannot tell them apart.
    let ready = spike.wait_for("RAWMODE_READY", Duration::from_secs(30));
    let mut observed = Vec::new();
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        if let Ok(mode) = unsafe { probe_input_mode(spike.pid) } {
            if observed.last() != Some(&mode) {
                observed.push(mode);
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    println!("program reported ready after {ready:?}");
    println!("distinct input modes observed while it held the keyboard: {observed:04x?}");

    assert!(
        ready.is_some(),
        "the raw-mode program must actually start, or this measures nothing; got:\n{}",
        spike.text()
    );
    // Not a startup race and not a stale handle: the probe *does* observe the child's
    // other changes to the mode, so if this flag were going to travel it would have.
    assert!(
        observed.len() > 1,
        "the probe must see the running program change the console mode at all, or it is \
         reading the wrong object and this measurement means nothing. Observed {observed:04x?}"
    );
    assert!(
        observed.iter().all(|m| m & ENABLE_PROCESSED_INPUT != 0),
        "FINDING REVERSED: processed input now clears through ConPTY when a program takes \
         raw control. Design D3 chose to ask the emulator because this did not work — if it \
         works now, that decision is worth revisiting. Observed {observed:04x?}"
    );
}
