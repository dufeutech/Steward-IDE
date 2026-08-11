# Terminal sessions

How an interactive shell reaches the screen. Describes what **is** — the code in
`app/src-tauri/src/core/terminal/`, `app/src-tauri/src/adapters/`, and
`app/packs/terminal/`.

The terminal is a second bounded context alongside [the asset-pack
system](asset-pack-system.md). The two meet in exactly two places: the composition root
(`lib.rs`), and the fact that the terminal's surface is delivered as a pack like any other.

## The shape

```mermaid
flowchart TB
    subgraph pack["terminal pack — pack:assets.terminal"]
        xterm["xterm.js 6<br/>+ fit + unicode11<br/><i>renders, parses VT, answers queries</i>"]
        surface["terminal.js<br/><i>panel, session wiring</i>"]
        surface --- xterm
    end

    subgraph root["composition root — lib.rs"]
        cmds["terminal_open / write / resize / interrupt / close / config<br/><i>thin: translate and delegate</i>"]
        gate{{"composition == application?"}}
        state["Sessions: Mutex&lt;Registry&gt;<br/>Spawner: Box&lt;dyn PtySpawner&gt;"]
    end

    subgraph core["core::terminal — pure, no I/O"]
        registry["Registry<br/><i>issues ids, refuses unknown/ended</i>"]
        session["Size · ExitCause · SessionError<br/><i>validates, classifies</i>"]
        port["`Pty` + `PtySpawner`<br/><b>ports</b>"]
        registry --- session
        registry --- port
    end

    subgraph adapters["adapters — everything that touches the OS"]
        ipc["terminal_ipc.rs<br/><i>Channel ↔ bytes, shell choice</i>"]
        ptyimpl["pty.rs — NativePtySpawner<br/><i>implements the ports</i>"]
        ctrl["console_ctrl.rs — Windows only<br/><i>raises CTRL_C_EVENT</i>"]
    end

    shell["OS shell<br/>pwsh · zsh · sh"]

    surface -- "invoke (raw bytes + Channel)" --> cmds
    cmds --> gate
    gate -- yes --> state
    state --> registry
    cmds -.-> ipc
    ipc -.-> port
    ptyimpl -- implements --> port
    ptyimpl -- "interrupt, on Windows" --> ctrl
    ptyimpl <== "PTY: ConPTY / openpty" ==> shell
    ctrl -- "console control event" --> shell

    classDef pure fill:#eef7ee,stroke:#4a7,stroke-width:1px
    classDef edge fill:#fff4e6,stroke:#d90,stroke-width:1px
    class core,registry,session,port pure
    class adapters,ipc,ptyimpl,ctrl edge
```

Dependencies point inward. `core::terminal` imports nothing from `adapters`, nothing from
the assets context, and no PTY library type ever crosses back into it — which is what makes
the whole core testable with no process spawned.

## Two paths, deliberately different

Bytes and facts do not travel the same way, and that is the load-bearing decision here.

```mermaid
sequenceDiagram
    participant S as terminal.js
    participant C as commands
    participant R as Registry
    participant A as pty.rs
    participant Sh as shell

    S->>C: terminal_open(cols, rows, Channel)
    C->>C: refuse unless composition == application
    C->>R: open(spawner, size, program)
    R->>A: spawn(SpawnRequest)
    A->>Sh: allocate PTY, spawn
    A-->>A: start reader + waiter threads
    R-->>C: SessionId
    C-->>S: id
    C--)S: event:terminal.session_started (bus, JSON)

    Sh--)A: output bytes
    A--)S: Channel: InvokeResponseBody::Raw
    Note over A,S: raw bytes — never JSON, never the bus

    S->>C: terminal_write(raw body, x-terminal-session header)
    C->>R: write(id, bytes)
    R->>Sh: bytes, unmodified

    Sh--)A: exits
    A-->>A: waiter reaps, classifies
    A->>R: mark_ended(id, cause)
    C--)S: event:terminal.session_exited (bus, JSON)
```

**Output bytes go down a per-session `Channel` as `InvokeResponseBody::Raw`; lifecycle facts
go on the app event bus as JSON.** Routing a build's output through the bus would force
base64 over the densest traffic in the application and make every listener parse a stream
that concerns one surface. The two event names are described in
[`schemas/terminal.events.asyncapi.yaml`](../../app/src-tauri/schemas/terminal.events.asyncapi.yaml);
output deliberately appears nowhere in that document, because it is not a domain fact.

## Interrupting is not a byte

Everything else about a session is byte-transparent in both directions. Interrupting is not,
because on Windows **no sequence of bytes written to the input stream becomes a control event
for a running command** — five candidate explanations for a missing conversion were refuted by
measurement before that conclusion was reached. So the interrupt is an operation of its own,
beside `write`, and it is the one place the adapter talks to the operating system rather than
to the terminal.

```mermaid
flowchart TB
    chord["interrupt chord in the surface"] --> obs["terminal.js reads<br/>buffer.active.type"]
    obs -- "reports, never decides" --> cmd["terminal_interrupt(session, fullScreen)"]
    cmd --> reg["Registry — unknown? ended?"]
    reg --> plat{{"platform"}}
    plat -- unix --> byte["write 0x03 to the PTY<br/><i>line discipline decides</i>"]
    plat -- windows --> pres{{"full-screen program?"}}
    pres -- yes --> byte2["write 0x03 to the PTY<br/><i>the chord is its input</i>"]
    pres -- no --> ev["console_ctrl: attach → guard →<br/>GenerateConsoleCtrlEvent → detach"]

    classDef edge fill:#fff4e6,stroke:#d90,stroke-width:1px
    class ev,byte,byte2 edge
```

Two things in that path are counter-intuitive enough to be worth stating, both measured:

- **The surface reports; the core decides.** Whether a full-screen program holds the keyboard
  is something only the emulator can see — the console will not report it through ConPTY
  (`ENABLE_PROCESSED_INPUT` stays set), and re-deriving it from the byte stream would put a
  second terminal emulator in the adapter. So `terminal.js` sends the observation and
  `core::terminal` decides what it means, exactly as it does with the size the surface reports.
- **The guard against our own event is a handler routine, not the documented ignore flag.**
  The event is delivered asynchronously, so the flag has to be cleared after a window that
  cannot be timed; and the flag is inherited by child processes, where a handler routine is
  not. Getting this wrong terminates the application instead of the command — it did, three
  times, before it was right.

Unix needs none of this: the line discipline already raises `SIGINT` for the foreground
process group, or hands the byte to a program that asked for raw input.

## Two threads per session, and why

```mermaid
flowchart LR
    spawn["spawn()"] --> reader["reader thread<br/>blocks in read()"]
    spawn --> waiter["waiter thread<br/>blocks in Child::wait()"]
    reader -- "bytes" --> sink["output sink → Channel"]
    waiter -- "ExitCause" --> exit["exit sink → mark_ended + event"]
    close["close()"] -- "ChildKiller::kill()" --> waiter
    close -- "drop master" --> reader
```

Exit is **not** inferred from the reader reaching EOF. Under ConPTY the master read side
stays open after the shell exits, so an EOF-only design reports no exit at all on Windows —
measured against a real shell, not theorised. Closing uses a `ChildKiller` cloned from the
child rather than locking it, because sharing one lock between the killer and the waiter
deadlocks: the killer would wait for a lock the waiter holds until the process it is waiting
for is killed.

## The execution boundary

A terminal is arbitrary code execution as the user — a strictly larger authority than
anything else the application grants. Three layers, none sufficient alone:

```mermaid
flowchart TB
    content["Pack content"] --> tuf{{"TUF-verified?<br/>hash-pinned per file"}}
    tuf -- no --> refused["never served"]
    tuf -- yes --> served["served from pack://localhost<br/>default-src 'self'"]
    served --> cap{{"capability grants<br/>allow-terminal-*?"}}
    cap -- no --> denied["command denied"]
    cap -- yes --> comp{{"composition == application?"}}
    comp -- "bootstrap" --> denied2["no session:<br/>recovery surface refused"]
    comp -- yes --> shell["session starts"]

    classDef stop fill:#fdecea,stroke:#c33
    class refused,denied,denied2 stop
```

1. **TUF verification is the primary control.** Content that could ask for a shell only gets
   in front of the user by being signed.
2. **Capability gating** — the commands are declared in `build.rs` via
   `AppManifest::commands`, which is what makes application-defined commands permissionable
   at all, and `capabilities/terminal.json` grants them to the `main` window.
3. **Composition gating** — layer 2 cannot do this job. Capabilities are scoped per window,
   and this app renders the bootstrap recovery surface and the application surface in the
   same `main` window, so `terminal_open` makes the distinction itself.

## Where each concern lives

| Concern                                              | Home                                                |
| ---------------------------------------------------- | --------------------------------------------------- |
| Which shell starts, scrollback bound                 | `config/app.config.json` → `core::terminal::config` |
| Session identity, size validity, exit classification | `core::terminal`                                    |
| PTY allocation, threads, reaping                     | `adapters/pty.rs`                                   |
| Raising a console control event (Windows)            | `adapters/console_ctrl.rs`                          |
| Channel and raw-body translation                     | `adapters/terminal_ipc.rs`                          |
| Wiring, gating, event emission                       | `lib.rs`                                            |
| Rendering, input encoding, scrollback                | `app/packs/terminal/`                               |

## Related

- [`asset-pack-system.md`](asset-pack-system.md) — how the surface is delivered and verified.
- [`DEV.md`](../../DEV.md) — running the app.
- [`docs/runbooks/pack-publishing.md`](../runbooks/pack-publishing.md) — publishing a pack.
- The change's [`design.md`](../../openspec/changes/terminal-surface/design.md) — the ADRs
  and the measurements behind them.
