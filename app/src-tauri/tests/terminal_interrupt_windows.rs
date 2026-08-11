#![cfg(windows)]
//! Re-opening the byte path, on evidence rather than on hope.
//!
//! `terminal-surface` D4c refuted five candidates for why writing `0x03` to a ConPTY never
//! becomes a control event, and this change's D2 concluded the byte path was a dead end.
//! Every one of those candidates looked for the fault in the *terminal* — the emulator, the
//! input mode, the ConPTY version, the shell.
//!
//! None of them looked at the **process that creates the pseudoconsole**, and that is where
//! the platform puts it. A process created with `CREATE_NEW_PROCESS_GROUP` carries an
//! "ignore Ctrl+C" attribute; the attribute is **inherited by every child**, so it travels
//! to the `conhost` that `CreatePseudoConsole` spawns, to the shell, and to the shell's own
//! children. Nothing on that console can then receive a control event — whether `conhost`
//! synthesises one from a byte, or another process raises one with
//! `GenerateConsoleCtrlEvent`. Which is exactly the reported symptom, on both paths.
//!
//! `SetConsoleCtrlHandler(NULL, FALSE)` clears the attribute for the calling process, and
//! must be called **before** the shell is spawned, since inheritance is fixed at creation.
//! Microsoft's own `node-pty` — the pseudoconsole layer behind VS Code's terminal — does
//! precisely this in `PtyStartProcess`, immediately after `CreatePseudoConsole` succeeds,
//! under the comment "Restore default handling of ctrl+c". `portable-pty` does not.
//!
//! This file measures that difference. One test, two phases, in one process and in order,
//! because the attribute being measured is process-global and phase two is what changes it.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

/// ConPTY asks the terminal where the cursor is and blocks until something answers. No
/// emulator here, so the harness answers (`terminal-surface` design D4b).
const CURSOR_QUERY: &[u8] = b"\x1b[6n";
const CURSOR_ANSWER: &[u8] = b"\x1b[1;1R";

/// The interrupt character a keyboard's chord puts on the wire.
const INTERRUPT: u8 = 0x03;

/// `ping -n 25` runs for ~21s, so "stopped" and "ran to completion" are never ambiguous.
const SETTLE: Duration = Duration::from_secs(3);

struct Shell {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    output: Arc<Mutex<Vec<u8>>>,
    child: Box<dyn Child + Send + Sync>,
    _master: Box<dyn MasterPty + Send>,
}

impl Shell {
    fn start(program: &str) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("a pseudoconsole can be allocated on this machine");

        let mut command = CommandBuilder::new(program);
        command.cwd(std::env::temp_dir());
        let child = pair
            .slave
            .spawn_command(command)
            .unwrap_or_else(|e| panic!("{program} must start on this machine: {e}"));
        drop(pair.slave);

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
    ///
    /// Two consecutive unchanged samples, for the reason `terminal_pty.rs` records: a command
    /// that has been echoed but has not yet produced output is indistinguishable from a
    /// settled shell, and one 250 ms sample lands in that gap often enough to matter.
    fn wait_until_quiet(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut last = usize::MAX;
        let mut unchanged = 0;
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(250));
            let seen = self.len();
            if seen > 0 && seen == last {
                unchanged += 1;
                if unchanged >= 2 {
                    return;
                }
            } else {
                unchanged = 0;
            }
            last = seen;
        }
        panic!("the shell never settled; got:\n{}", self.text());
    }

    fn start_a_long_command(&self) {
        self.wait_until_quiet();
        let before = self.len();
        self.write(b"ping -n 25 127.0.0.1\r\n");
        std::thread::sleep(SETTLE);
        assert!(
            self.len() > before,
            "the long command must be running before the interrupt is measured; got:\n{}",
            self.text()
        );
    }

    /// How many replies `ping` has printed. Counting the command's own output rather than a
    /// typed marker: PSReadLine rewrites what is typed at it, and `ping`'s prose is
    /// localised, but `bytes=32` is neither.
    fn replies(&self) -> usize {
        self.text().matches("bytes=32").count()
    }

    /// Write the interrupt byte, let things settle, and report whether the command stopped.
    fn interrupted_by_the_byte(&self) -> (bool, usize, usize) {
        self.write(&[INTERRUPT]);
        let at_interrupt = self.replies();
        std::thread::sleep(SETTLE);
        let after = self.replies();
        (after == at_interrupt && after < 10, at_interrupt, after)
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// # Safety
/// FFI, and process-global: it clears this process's inherited "ignore Ctrl+C" attribute.
/// Called once, between the two phases below, which is the whole point of the measurement.
unsafe fn restore_default_ctrl_c_handling() {
    // `NULL` with `FALSE` means "stop ignoring Ctrl+C", the inverse of the ignore attribute
    // `CREATE_NEW_PROCESS_GROUP` sets and children inherit.
    let ok = SetConsoleCtrlHandler(None, 0);
    assert_ne!(ok, 0, "restoring default Ctrl+C handling must succeed");
}

#[test]
fn the_byte_interrupts_a_running_command_in_an_ordinary_process() {
    // The control, and on its own a correction: `terminal-surface` D4c concluded that no
    // byte written to a ConPTY ever becomes a control event on this machine. It does. Both
    // shells, this crate's pseudoconsole crate, no console attachment anywhere.
    for program in ["cmd.exe", "powershell.exe"] {
        let shell = Shell::start(program);
        shell.start_a_long_command();
        let (stopped, before, after) = shell.interrupted_by_the_byte();
        println!("{program}: byte → replies {before} → {after}, stopped={stopped}");
        assert!(
            stopped,
            "writing {INTERRUPT:#04x} must stop the running command — replies went \
             {before} → {after} under {program}; got:\n{}",
            shell.text()
        );
    }
}

/// The application's condition, reproduced: the process that creates the pseudoconsole is
/// itself the root of a new process group, so it carries the "ignore Ctrl+C" attribute and
/// every child it makes inherits it.
///
/// Run as a child of the test below rather than as a test in its own right, because the
/// attribute cannot be set on a process that is already running — only inherited at
/// creation. `STEWARD_SPIKE_RESTORE` selects whether the fix is applied before the spawn.
#[test]
#[ignore = "re-entered as a child process by the test below; not a test on its own"]
fn spike_inner_measure_the_byte_in_this_process() {
    if std::env::var_os("STEWARD_SPIKE_RESTORE").is_some() {
        unsafe { restore_default_ctrl_c_handling() };
    }
    let shell = Shell::start("powershell.exe");
    shell.start_a_long_command();
    let (stopped, before, after) = shell.interrupted_by_the_byte();
    println!("inner: byte → replies {before} → {after}, stopped={stopped}");
    assert!(stopped, "the running command must stop");
}

/// Did the inner measurement stop the command? `false` means the byte evaporated.
fn measure_in_a_new_process_group(restore: bool) -> bool {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    let mut command = std::process::Command::new(
        std::env::current_exe().expect("the test binary can name itself"),
    );
    command
        .args([
            "--ignored",
            "--exact",
            "--nocapture",
            "--test-threads=1",
            "spike_inner_measure_the_byte_in_this_process",
        ])
        .creation_flags(CREATE_NEW_PROCESS_GROUP);
    if restore {
        command.env("STEWARD_SPIKE_RESTORE", "1");
    } else {
        command.env_remove("STEWARD_SPIKE_RESTORE");
    }

    let output = command.output().expect("the inner measurement can be run");
    println!(
        "--- inner (restore={restore}) exit {:?} ---\n{}{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output.status.success()
}

#[test]
fn a_new_process_group_is_what_breaks_the_interrupt_and_restoring_the_attribute_fixes_it() {
    // The variable D2a narrowed to — "the pseudoconsole the application created, or
    // something about the process that created it" — named at last. `CREATE_NEW_PROCESS_GROUP`
    // is documented as disabling Ctrl+C "for all processes within the new process group",
    // and the attribute is inherited, so it reaches the `conhost` that `CreatePseudoConsole`
    // spawns and every process on that pseudoconsole.
    let broken = measure_in_a_new_process_group(false);
    let fixed = measure_in_a_new_process_group(true);

    assert!(
        !broken,
        "a pseudoconsole created from a new process group must NOT be interruptible — if it \
         is, the application's failure has some other cause and this explanation is refuted"
    );
    assert!(
        fixed,
        "clearing the inherited ignore attribute with SetConsoleCtrlHandler(NULL, FALSE) \
         before the spawn must restore the interrupt — this is the call node-pty makes in \
         PtyStartProcess and the one portable-pty omits"
    );
}
