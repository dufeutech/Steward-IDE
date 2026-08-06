// Bootstrap recovery surface (spec bootstrap-shell): the surface shown while no
// application pack can be served. It reports acquisition state, offers a retry, and shows
// diagnostics — nothing else. It is deliberately plain DOM with no framework and no build
// step, because it has to work when everything else is broken.
//
// Everything it needs is embedded in the binary; it never reaches a remote origin.

(() => {
  const doc = document;

  const el = (tag, className, text) => {
    const node = doc.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  };

  // Own the surface: the shell template's application container is not ours to use.
  const editor = doc.getElementById("editor");
  if (editor) editor.hidden = true;
  const shellStatus = doc.getElementById("status");
  if (shellStatus) shellStatus.hidden = true;

  const root = el("section", "boot");
  const title = el("h1", "boot__title", "Steward IDE");
  const status = el("p", "boot__status", "Preparing…");
  const bar = el("div", "boot__bar");
  const fill = el("div", "boot__fill");
  bar.appendChild(fill);
  const actions = el("div", "boot__actions");
  const retry = el("button", null, "Retry");
  const toggle = el("button", null, "Show details");
  const details = el("pre", "boot__details");
  const copy = el("button", null, "Copy details");

  retry.hidden = true;
  copy.hidden = true;
  details.hidden = true;
  actions.append(retry, toggle, copy);
  root.append(title, status, bar, actions, details);
  doc.body.appendChild(root);

  const diagnostics = [];
  const note = (line) => {
    diagnostics.push(line);
    details.textContent = diagnostics.join("\n");
  };

  const mib = (bytes) => (bytes / 1048576).toFixed(1) + " MiB";

  // --- state transitions -----------------------------------------------------------

  const acquiring = (done, total) => {
    retry.hidden = true;
    bar.hidden = false;
    const pct = total > 0 ? Math.min(100, (done / total) * 100) : 0;
    fill.style.width = pct + "%";
    status.textContent =
      total > 0
        ? `Fetching application content — ${mib(done)} of ${mib(total)}`
        : "Fetching application content…";
  };

  const REASONS = {
    verification: "Application content was rejected as unverified.",
    local: "Application content could not be stored on this machine.",
    transport: "Application content could not be downloaded.",
  };

  const failed = (kind, message) => {
    bar.hidden = true;
    retry.hidden = false;
    status.textContent = REASONS[kind] || REASONS.transport;
    note(`${kind}: ${message}`);
  };

  const activated = (pack, version) => {
    bar.hidden = true;
    retry.hidden = true;
    fill.style.width = "100%";
    status.textContent = `Loading ${pack} ${version}…`;
  };

  // --- actions ---------------------------------------------------------------------

  toggle.addEventListener("click", () => {
    details.hidden = !details.hidden;
    copy.hidden = details.hidden;
    toggle.textContent = details.hidden ? "Show details" : "Hide details";
  });

  copy.addEventListener("click", () => {
    // Best-effort: the clipboard is a convenience, never the only way to read this.
    if (navigator.clipboard) navigator.clipboard.writeText(details.textContent);
  });

  // The command is invoked directly; no handle is parked on `window` for something else
  // to find — capabilities are granted explicitly, never discovered ambiently (design D8).
  retry.addEventListener("click", () => {
    status.textContent = "Retrying…";
    retry.hidden = true;
    bar.hidden = false;
    fill.style.width = "0";
    const host = window.__TAURI__;
    if (!host) return;
    host.core.invoke("retry_acquisition").catch((e) => failed("transport", String(e)));
  });

  // --- acquisition state: observed, never polled (design D5) -------------------------

  const tauri = window.__TAURI__;
  if (!tauri) {
    // The surface still renders and still explains itself — that is the whole point of
    // it being embedded — it just has nothing to report.
    status.textContent = "Application content is unavailable.";
    note("no host bridge available: acquisition state cannot be observed");
    return;
  }

  note(`started ${new Date().toISOString()}`);
  status.textContent = "Fetching application content…";

  tauri.event.listen("event:assets.acquisition_progressed", (e) => {
    acquiring(e.payload.done_bytes, e.payload.total_bytes);
  });

  tauri.event.listen("event:assets.acquisition_failed", (e) => {
    failed(e.payload.kind, e.payload.reason);
  });

  tauri.event.listen("event:assets.pack_activated", (e) => {
    // The surface yields the moment there is something better to show.
    activated(e.payload.pack, e.payload.version);
    window.location.reload();
  });
})();
