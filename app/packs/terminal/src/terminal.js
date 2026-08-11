// The terminal surface (spec `terminal-surface`; change `terminal-surface`).
//
// Delivered as pack content, so this file is served from the pack origin under
// `script-src 'self'` — there is no inline script and no remote origin anywhere in it.
//
// Everything about *rendering* a terminal is xterm.js's job (ADR "Terminal emulation and
// rendering"). What this file owns is the wiring: one session, its bytes, its size, and
// what the surface shows when there is no session to show.

import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
// Imported here so the build emits one stylesheet next to one script; the manifest lists
// both as entry points and the shell generates the tags.
import "./terminal.css";

/// The composition marker the shell stamps on `<body>`. A terminal is application
/// functionality, and the bootstrap recovery surface offers none (spec `terminal-surface`:
/// "Application content is unavailable"). The backend refuses too — this is the surface
/// side of the same rule, so a recovery page never even asks.
const APPLICATION = "application";

/// Toggling deliberately avoids Ctrl+` : that chord reaches the shell in a real terminal,
/// and the spec forbids the surface consuming a key the session needs. Ctrl+Shift+`
/// is not a sequence any shell reads.
const TOGGLE = (e) =>
  e.ctrlKey && e.shiftKey && (e.code === "Backquote" || e.key === "~");

/// The interrupt chord. Routed through `terminal_interrupt` rather than as `\x03` through
/// `terminal_write`, because the specification names interrupting as an operation with a
/// refusal of its own. What it sends is the same `0x03` any terminal sends; the platform
/// decides what that means — see the `terminal-interrupt-signal` change, design D2.
///
/// Shift is excluded deliberately: Ctrl+Shift+C is a different chord and must not be
/// swallowed here. This does *not* bind Ctrl+C to a surface action — the chord still goes
/// to the session, which is what the spec forbids the surface from preventing.
const INTERRUPT = (e) =>
  e.ctrlKey &&
  !e.shiftKey &&
  !e.altKey &&
  !e.metaKey &&
  (e.code === "KeyC" || e.key === "c" || e.key === "C");

const encoder = new TextEncoder();

function tauri() {
  const api = globalThis.__TAURI__;
  if (!api) throw new Error("the Tauri bridge is unavailable");
  return api.core;
}

/// One terminal panel and the session behind it.
class TerminalSurface {
  constructor(root) {
    this.root = root;
    this.sessionId = null;
    this.term = null;
    this.fit = null;
    this.disposers = [];

    this.panel = document.createElement("section");
    this.panel.className = "steward-terminal";
    this.panel.hidden = true;
    this.panel.innerHTML = [
      '<header class="steward-terminal__bar">',
      '  <span class="steward-terminal__title">Terminal</span>',
      '  <span class="steward-terminal__state" data-role="state"></span>',
      '  <button class="steward-terminal__action" data-role="restart" hidden>New session</button>',
      '  <button class="steward-terminal__action" data-role="hide" title="Ctrl+Shift+`">Hide</button>',
      "</header>",
      '<div class="steward-terminal__screen" data-role="screen"></div>',
    ].join("");

    this.screen = this.panel.querySelector('[data-role="screen"]');
    this.state = this.panel.querySelector('[data-role="state"]');
    this.restartButton = this.panel.querySelector('[data-role="restart"]');
    this.restartButton.addEventListener("click", () => void this.restart());
    this.panel
      .querySelector('[data-role="hide"]')
      .addEventListener("click", () => this.toggle(false));

    this.toggleButton = document.createElement("button");
    this.toggleButton.className = "steward-terminal__toggle";
    this.toggleButton.textContent = "Terminal";
    this.toggleButton.title = "Ctrl+Shift+`";
    this.toggleButton.addEventListener("click", () => this.toggle());

    root.append(this.toggleButton, this.panel);

    // Resize is reported from the panel itself, not from the window: the terminal can
    // change size without the window doing so (spec: "keeps the session's size in step").
    this.observer = new ResizeObserver(() => this.syncSize());
    this.observer.observe(this.panel);

    window.addEventListener("keydown", (e) => {
      if (TOGGLE(e)) {
        e.preventDefault();
        this.toggle();
      }
    });
  }

  /// Show or hide. Dismissing never ends the session — the shell keeps running and its
  /// output keeps landing in the scrollback (spec: "dismissed and restored").
  toggle(force) {
    const show = force ?? this.panel.hidden;
    this.panel.hidden = !show;
    this.toggleButton.setAttribute("aria-expanded", String(show));
    if (show) {
      if (!this.term) void this.start();
      else {
        this.syncSize();
        this.term.focus();
      }
    }
  }

  say(text, { canRestart = false } = {}) {
    this.state.textContent = text;
    this.restartButton.hidden = !canRestart;
  }

  async start() {
    let scrollback = 1000;
    try {
      const config = await tauri().invoke("terminal_config");
      scrollback = config.scrollback_lines;
    } catch (err) {
      // A missing config is not fatal to rendering; the bound falls back to the same
      // number the config ships. Worth saying so rather than silently differing.
      console.warn("terminal: using the default scrollback bound:", err);
    }

    const term = new Terminal({
      allowProposedApi: true,
      scrollback,
      convertEol: false,
      cursorBlink: true,
      fontFamily:
        'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
      fontSize: 13,
      // New output must not yank the viewport away from someone reading scrollback
      // (spec: "New output MUST NOT silently move the viewport").
      scrollOnUserInput: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new Unicode11Addon());
    // Unicode 11 widths are why double-width text stays aligned. Activating is a separate
    // step from loading — loading alone changes nothing.
    term.unicode.activeVersion = "11";

    // Every key belongs to the session except the one chord that shows and hides this
    // panel (spec: "routes input to the session without intercepting it").
    term.attachCustomKeyEventHandler((e) => {
      if (TOGGLE(e)) return false;
      // Keydown only, and returning false so xterm does not *also* emit `\x03`: the spec
      // requires the chord reach the session exactly once.
      if (e.type === "keydown" && INTERRUPT(e)) {
        void this.interrupt();
        return false;
      }
      return true;
    });

    term.open(this.screen);
    this.term = term;
    this.fit = fit;

    // `measure()` yields nothing until the panel has a layout, and web fonts can land a
    // frame late. Falling back to what the terminal already believes it is keeps the
    // session's opening size honest instead of dereferencing null before the try below.
    const size = this.measure() ?? { cols: term.cols, rows: term.rows };
    const onOutput = new (globalThis.__TAURI__.core.Channel)();
    // Bytes, not text: xterm decodes UTF-8 itself and carries a multi-byte character
    // split across two messages, which a string round trip would corrupt.
    onOutput.onmessage = (message) => term.write(new Uint8Array(message));

    try {
      this.sessionId = await tauri().invoke("terminal_open", {
        columns: size.cols,
        rows: size.rows,
        onOutput,
      });
    } catch (err) {
      // Spec: a surface whose session could not be started states the reason and does not
      // present an empty terminal that silently swallows what is typed into it.
      this.sessionId = null;
      term.options.disableStdin = true;
      term.writeln(`\x1b[31mNo terminal session: ${String(err)}\x1b[0m`);
      this.say("could not start", { canRestart: true });
      return;
    }

    this.say("");
    this.disposers.push(
      term.onData((data) => this.send(encoder.encode(data))),
      // Binary is what xterm emits for input that is not valid UTF-16 text; it is already
      // a byte string, so it must not go through the encoder a second time.
      term.onBinary((data) => {
        const bytes = new Uint8Array(data.length);
        for (let i = 0; i < data.length; i++) bytes[i] = data.charCodeAt(i) & 0xff;
        this.send(bytes);
      }),
      term.onResize(() => this.reportSize()),
    );

    this.listenForExit();
    this.syncSize();
    term.focus();
  }

  async listenForExit() {
    const { event } = globalThis.__TAURI__;
    const unlisten = await event.listen("event:terminal.session_exited", ({ payload }) => {
      if (payload.session_id !== this.sessionId) return;
      this.ended(payload);
    });
    this.disposers.push({ dispose: unlisten });
  }

  /// Spec: when the session ends, say so and why, stop presenting as accepting input, and
  /// offer a new one.
  ended(payload) {
    const detail =
      payload.cause === "exited"
        ? `exited with status ${payload.code}`
        : payload.detail
          ? `${payload.cause}: ${payload.detail}`
          : payload.cause;
    this.sessionId = null;
    if (this.term) {
      this.term.options.disableStdin = true;
      this.term.options.cursorBlink = false;
      this.term.writeln(`\r\n\x1b[90m[session ${detail}]\x1b[0m`);
    }
    this.say(detail, { canRestart: true });
  }

  async restart() {
    this.dispose();
    this.screen.replaceChildren();
    this.say("");
    await this.start();
  }

  send(bytes) {
    if (this.sessionId === null || bytes.length === 0) return;
    // The body is the bytes themselves, so the session travels in a header — the raw
    // path has nowhere else to put it (see `adapters/terminal_ipc.rs`).
    tauri()
      .invoke("terminal_write", bytes, {
        headers: { "x-terminal-session": String(this.sessionId) },
      })
      .catch((err) => console.warn("terminal: write refused:", err));
  }

  /// Ask the session to interrupt what it is running.
  ///
  /// Nothing is reported with it. The chord is an operation on the session, and what it
  /// means for the program currently running is the platform's decision, made where the
  /// program's input actually arrives — the line discipline on Unix, `conhost` on Windows.
  /// A full-screen program that has taken raw control receives the chord as input from
  /// both, so this side has nothing to observe on its behalf (design D2b).
  interrupt() {
    if (this.sessionId === null || !this.term) return;
    tauri()
      .invoke("terminal_interrupt", { session: this.sessionId })
      .catch((err) => console.warn("terminal: interrupt refused:", err));
  }

  measure() {
    const proposed = this.fit?.proposeDimensions();
    // `proposeDimensions` returns nothing while the panel is hidden or has no layout yet.
    // Reporting a size we cannot present would tell the shell to lay out to a viewport
    // that does not exist (spec: "The terminal is not visible").
    if (!proposed || !proposed.cols || !proposed.rows) return null;
    if (!Number.isFinite(proposed.cols) || !Number.isFinite(proposed.rows)) return null;
    return proposed;
  }

  syncSize() {
    if (!this.term || this.panel.hidden) return;
    const size = this.measure();
    if (!size) return;
    // `fit` resizes the terminal, which fires `onResize`, which reports to the session.
    this.fit.fit();
  }

  reportSize() {
    if (this.sessionId === null || !this.term) return;
    tauri()
      .invoke("terminal_resize", {
        session: this.sessionId,
        columns: this.term.cols,
        rows: this.term.rows,
      })
      .catch((err) => console.warn("terminal: resize refused:", err));
  }

  dispose() {
    for (const d of this.disposers) d.dispose?.();
    this.disposers = [];
    if (this.sessionId !== null) {
      const session = this.sessionId;
      this.sessionId = null;
      tauri()
        .invoke("terminal_close", { session })
        .catch(() => {});
    }
    this.term?.dispose();
    this.term = null;
    this.fit = null;
  }
}

function mount() {
  if (document.body.dataset.composition !== APPLICATION) return;
  const host = document.createElement("div");
  host.className = "steward-terminal-host";
  document.body.append(host);
  const surface = new TerminalSurface(host);
  // Closing the window should not depend on the backend's exit sweep alone.
  window.addEventListener("pagehide", () => surface.dispose());
  // Deliberately no window global holding the surface: capabilities are granted, never
  // discovered ambiently (the same rule the editor follows).
}

if (document.readyState === "loading") {
  window.addEventListener("DOMContentLoaded", mount);
} else {
  mount();
}
