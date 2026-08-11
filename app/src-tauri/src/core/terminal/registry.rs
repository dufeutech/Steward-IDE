//! The live-session registry (spec `terminal-session`; design D5).
//!
//! Pure: it holds `Box<dyn Pty>` values and enforces the addressing rules. It spawns
//! nothing, knows no framework, and is exercised in tests against a fake `Pty`.

use std::collections::HashMap;

use super::session::{
    ExitCause, Presenting, Pty, PtySpawner, SessionError, SessionId, Size, SpawnRequest,
};

struct Entry {
    pty: Box<dyn Pty>,
    /// Set when the shell stops. The entry outlives the shell on purpose: a surface that
    /// writes to a just-exited session must be told *"ended"*, not *"unknown"*, and that
    /// distinction is only possible if the identifier is still remembered.
    ended: Option<ExitCause>,
}

/// Every live session, addressed by an identifier this type alone issues.
#[derive(Default)]
pub struct Registry {
    next: u64,
    sessions: HashMap<SessionId, Entry>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Issue the next identifier. Monotonic and never reused, including across closes —
    /// reuse is what would let a stale handle address a stranger's session.
    fn issue(&mut self) -> SessionId {
        self.next += 1;
        SessionId::new(self.next)
    }

    /// Start a session and remember it. The identifier is issued *after* the spawn
    /// succeeds, so a failed start burns no identifier and leaves no entry behind.
    pub fn open(
        &mut self,
        spawner: &dyn PtySpawner,
        size: Size,
        program: String,
        on_output: impl Fn(&[u8]) + Send + 'static,
        on_exit: impl FnOnce(SessionId, ExitCause) + Send + 'static,
    ) -> Result<SessionId, SessionError> {
        // Peek at the identifier so the exit callback can name its own session without a
        // second round trip; it only becomes real if the spawn below succeeds.
        let id = SessionId::new(self.next + 1);
        let pty = spawner.spawn(SpawnRequest {
            program,
            size,
            on_output: Box::new(on_output),
            on_exit: Box::new(move |cause| on_exit(id, cause)),
        })?;
        let issued = self.issue();
        debug_assert_eq!(issued, id);
        self.sessions.insert(issued, Entry { pty, ended: None });
        Ok(issued)
    }

    fn live_mut(&mut self, id: SessionId) -> Result<&mut Entry, SessionError> {
        match self.sessions.get_mut(&id) {
            None => Err(SessionError::Unknown(id)),
            Some(entry) if entry.ended.is_some() => Err(SessionError::Ended(id)),
            Some(entry) => Ok(entry),
        }
    }

    pub fn write(&mut self, id: SessionId, bytes: &[u8]) -> Result<(), SessionError> {
        self.live_mut(id)?.pty.write(bytes)
    }

    pub fn resize(&mut self, id: SessionId, size: Size) -> Result<(), SessionError> {
        self.live_mut(id)?.pty.resize(size)
    }

    /// Interrupt what one session is running. Routed through `live_mut` like every other
    /// operation, so "no such session" and "that session has ended" are decided in one
    /// place rather than restated here.
    pub fn interrupt(&mut self, id: SessionId, presenting: Presenting) -> Result<(), SessionError> {
        self.live_mut(id)?.pty.interrupt(presenting)
    }

    /// Close a session and forget it. Closing an already-ended session succeeds: the
    /// shell exiting first is the common case, not an error the surface should handle.
    pub fn close(&mut self, id: SessionId) -> Result<(), SessionError> {
        let mut entry = self.sessions.remove(&id).ok_or(SessionError::Unknown(id))?;
        entry.pty.close()
    }

    /// Record that a session's shell stopped. Called from the composition root when the
    /// adapter reports an exit; the entry stays so later writes report `Ended`.
    pub fn mark_ended(&mut self, id: SessionId, cause: ExitCause) {
        if let Some(entry) = self.sessions.get_mut(&id) {
            entry.ended.get_or_insert(cause);
        }
    }

    pub fn is_ended(&self, id: SessionId) -> bool {
        self.sessions
            .get(&id)
            .map(|e| e.ended.is_some())
            .unwrap_or(true)
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Terminate every session. The application exiting must leave nothing running
    /// (spec scenario "The application exits with sessions open"), so this ignores
    /// individual failures and keeps going rather than stopping at the first.
    pub fn close_all(&mut self) {
        for (_, mut entry) in self.sessions.drain() {
            let _ = entry.pty.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A `Pty` that records what it was told, so the registry's rules can be exercised
    /// with no process, no thread, and no platform dependency.
    #[derive(Default)]
    struct Recorder {
        written: Vec<u8>,
        sizes: Vec<Size>,
        interrupts: Vec<Presenting>,
        closed: bool,
    }

    #[derive(Clone, Default)]
    struct FakePty(Arc<Mutex<Recorder>>);

    impl Pty for FakePty {
        fn write(&mut self, bytes: &[u8]) -> Result<(), SessionError> {
            self.0.lock().unwrap().written.extend_from_slice(bytes);
            Ok(())
        }
        fn resize(&mut self, size: Size) -> Result<(), SessionError> {
            self.0.lock().unwrap().sizes.push(size);
            Ok(())
        }
        fn interrupt(&mut self, presenting: Presenting) -> Result<(), SessionError> {
            self.0.lock().unwrap().interrupts.push(presenting);
            Ok(())
        }
        fn close(&mut self) -> Result<(), SessionError> {
            self.0.lock().unwrap().closed = true;
            Ok(())
        }
    }

    /// Hands out a fresh `FakePty` per spawn, keeps a handle to each, and parks the exit
    /// sinks so a test can fire one the way a reader thread would.
    #[derive(Default)]
    struct FakeSpawner {
        made: Mutex<Vec<FakePty>>,
        exits: Mutex<Vec<super::super::session::ExitSink>>,
        fail_with: Option<String>,
    }

    impl FakeSpawner {
        fn failing(reason: &str) -> Self {
            Self {
                fail_with: Some(reason.to_string()),
                ..Default::default()
            }
        }
        fn nth(&self, i: usize) -> FakePty {
            self.made.lock().unwrap()[i].clone()
        }
        /// Stand in for the shell backing session `i` stopping.
        fn fire_exit(&self, i: usize, cause: ExitCause) {
            let sink = self.exits.lock().unwrap().remove(i);
            sink(cause);
        }
    }

    impl PtySpawner for FakeSpawner {
        fn spawn(&self, request: SpawnRequest) -> Result<Box<dyn Pty>, SessionError> {
            if let Some(reason) = &self.fail_with {
                return Err(SessionError::Spawn(reason.clone()));
            }
            let pty = FakePty::default();
            self.made.lock().unwrap().push(pty.clone());
            self.exits.lock().unwrap().push(request.on_exit);
            // Prove the output sink is wired without needing a real shell.
            (request.on_output)(b"ready");
            Ok(Box::new(pty))
        }
    }

    fn size() -> Size {
        Size::new(80, 24).unwrap()
    }

    fn open(registry: &mut Registry, spawner: &FakeSpawner) -> Result<SessionId, SessionError> {
        registry.open(spawner, size(), "sh".into(), |_| {}, |_, _| {})
    }

    #[test]
    fn scenario_an_unknown_session_is_addressed() {
        let mut registry = Registry::new();
        let ghost = SessionId::new(999);
        assert_eq!(
            registry.write(ghost, b"x"),
            Err(SessionError::Unknown(ghost))
        );
        assert_eq!(
            registry.resize(ghost, size()),
            Err(SessionError::Unknown(ghost))
        );
        assert_eq!(
            registry.interrupt(ghost, Presenting::Normally),
            Err(SessionError::Unknown(ghost))
        );
        assert_eq!(registry.close(ghost), Err(SessionError::Unknown(ghost)));
    }

    #[test]
    fn scenario_only_the_addressed_session_is_interrupted() {
        let spawner = FakeSpawner::default();
        let mut registry = Registry::new();
        let first = open(&mut registry, &spawner).unwrap();
        open(&mut registry, &spawner).unwrap();

        registry.interrupt(first, Presenting::Normally).unwrap();

        assert_eq!(
            spawner.nth(0).0.lock().unwrap().interrupts,
            vec![Presenting::Normally]
        );
        assert!(
            spawner.nth(1).0.lock().unwrap().interrupts.is_empty(),
            "the other session's command keeps running"
        );
    }

    #[test]
    fn what_the_surface_reports_reaches_the_pty_unchanged() {
        // The core does not decide delivery, so it must not quietly normalise the
        // observation on the way through either (design D3).
        let spawner = FakeSpawner::default();
        let mut registry = Registry::new();
        let id = open(&mut registry, &spawner).unwrap();

        registry.interrupt(id, Presenting::FullScreen).unwrap();
        registry.interrupt(id, Presenting::Normally).unwrap();

        assert_eq!(
            spawner.nth(0).0.lock().unwrap().interrupts,
            vec![Presenting::FullScreen, Presenting::Normally]
        );
    }

    #[test]
    fn scenario_interrupting_a_session_that_has_ended() {
        let spawner = FakeSpawner::default();
        let mut registry = Registry::new();
        let id = open(&mut registry, &spawner).unwrap();
        registry.mark_ended(id, ExitCause::Exited { code: 0 });

        assert_eq!(
            registry.interrupt(id, Presenting::Normally),
            Err(SessionError::Ended(id)),
            "an interrupt after the session ended is refused with a reason, not ignored"
        );
        assert!(
            spawner.nth(0).0.lock().unwrap().interrupts.is_empty(),
            "and no process is signalled"
        );
    }

    #[test]
    fn scenario_two_sessions_run_concurrently() {
        let spawner = FakeSpawner::default();
        let mut registry = Registry::new();
        let first = open(&mut registry, &spawner).unwrap();
        let second = open(&mut registry, &spawner).unwrap();
        assert_ne!(first, second);

        registry.write(first, b"to-first").unwrap();

        assert_eq!(spawner.nth(0).0.lock().unwrap().written, b"to-first");
        assert!(
            spawner.nth(1).0.lock().unwrap().written.is_empty(),
            "the other session's state is unchanged"
        );
    }

    #[test]
    fn scenario_input_after_the_session_ended() {
        let spawner = FakeSpawner::default();
        let mut registry = Registry::new();
        let id = open(&mut registry, &spawner).unwrap();
        registry.mark_ended(id, ExitCause::Exited { code: 0 });

        // "Ended", never "unknown" — the surface must be able to tell the two apart.
        assert_eq!(registry.write(id, b"x"), Err(SessionError::Ended(id)));
        assert_eq!(registry.resize(id, size()), Err(SessionError::Ended(id)));
        assert!(registry.is_ended(id));
    }

    #[test]
    fn closing_an_ended_session_succeeds() {
        // The shell exiting before the surface closes it is the ordinary case.
        let spawner = FakeSpawner::default();
        let mut registry = Registry::new();
        let id = open(&mut registry, &spawner).unwrap();
        registry.mark_ended(id, ExitCause::Exited { code: 0 });
        assert_eq!(registry.close(id), Ok(()));
        assert_eq!(registry.close(id), Err(SessionError::Unknown(id)));
    }

    #[test]
    fn identifiers_are_never_reused() {
        let spawner = FakeSpawner::default();
        let mut registry = Registry::new();
        let first = open(&mut registry, &spawner).unwrap();
        registry.close(first).unwrap();
        let second = open(&mut registry, &spawner).unwrap();
        assert_ne!(
            first, second,
            "a closed identifier must never name a later session"
        );
        assert_eq!(
            registry.write(first, b"x"),
            Err(SessionError::Unknown(first))
        );
    }

    #[test]
    fn a_failed_spawn_leaves_nothing_behind() {
        let spawner = FakeSpawner::failing("no shell on this machine");
        let mut registry = Registry::new();
        assert_eq!(
            open(&mut registry, &spawner),
            Err(SessionError::Spawn("no shell on this machine".into()))
        );
        assert!(registry.is_empty());

        // And the burned identifier is not skipped: the next session gets the first one.
        let working = FakeSpawner::default();
        assert_eq!(open(&mut registry, &working).unwrap().get(), 1);
    }

    #[test]
    fn scenario_the_application_exits_with_sessions_open() {
        let spawner = FakeSpawner::default();
        let mut registry = Registry::new();
        open(&mut registry, &spawner).unwrap();
        open(&mut registry, &spawner).unwrap();

        registry.close_all();

        assert!(registry.is_empty());
        assert!(spawner.nth(0).0.lock().unwrap().closed);
        assert!(spawner.nth(1).0.lock().unwrap().closed);
    }

    #[test]
    fn resize_reaches_the_pty() {
        let spawner = FakeSpawner::default();
        let mut registry = Registry::new();
        let id = open(&mut registry, &spawner).unwrap();
        let bigger = Size::new(120, 40).unwrap();
        registry.resize(id, bigger).unwrap();
        assert_eq!(spawner.nth(0).0.lock().unwrap().sizes, vec![bigger]);
    }

    #[test]
    fn the_exit_sink_names_its_own_session() {
        // The adapter reports an exit by calling the sink; it must carry the identifier
        // the registry issued, or the composition root marks the wrong session ended.
        let spawner = FakeSpawner::default();
        let mut registry = Registry::new();
        let seen: Arc<Mutex<Vec<(SessionId, ExitCause)>>> = Arc::default();

        let open_watched = |registry: &mut Registry| {
            let sink = seen.clone();
            registry
                .open(
                    &spawner,
                    size(),
                    "sh".into(),
                    |_| {},
                    move |id, cause| sink.lock().unwrap().push((id, cause)),
                )
                .unwrap()
        };
        let first = open_watched(&mut registry);
        let second = open_watched(&mut registry);

        spawner.fire_exit(1, ExitCause::Exited { code: 3 });

        assert_eq!(
            *seen.lock().unwrap(),
            vec![(second, ExitCause::Exited { code: 3 })],
            "the second session's exit must name the second session"
        );
        assert_ne!(first, second);
    }

    #[test]
    fn an_exit_reported_for_a_closed_session_is_harmless() {
        // The shell can stop while the close is in flight; marking a forgotten session
        // must not resurrect it or panic.
        let spawner = FakeSpawner::default();
        let mut registry = Registry::new();
        let id = open(&mut registry, &spawner).unwrap();
        registry.close(id).unwrap();
        registry.mark_ended(id, ExitCause::Exited { code: 0 });
        assert!(registry.is_empty());
    }

    #[test]
    fn the_first_reported_cause_is_the_one_kept() {
        // A kill that races the PTY tearing down must not be relabelled by the second
        // report to arrive.
        let spawner = FakeSpawner::default();
        let mut registry = Registry::new();
        let id = open(&mut registry, &spawner).unwrap();
        registry.mark_ended(
            id,
            ExitCause::Signalled {
                signal: Some("SIGKILL".into()),
            },
        );
        registry.mark_ended(
            id,
            ExitCause::Failed {
                reason: "pty closed".into(),
            },
        );
        assert_eq!(registry.write(id, b"x"), Err(SessionError::Ended(id)));
    }
}
