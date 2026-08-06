// Shell bootstrap. External file on purpose: inline scripts are dead under the
// enforced CSP (script-src 'self'). Everything heavy comes from the asset packs.
window.addEventListener("DOMContentLoaded", () => {
  const status = document.getElementById("status");
  // The shell hosts whichever surface the core composed. The bootstrap pack owns its own
  // surface and its own boot signalling; driving the application pack is this file's only
  // job, so it stands down rather than inferring anything from a missing global.
  if (document.body.dataset.composition !== "application") return;
  try {
    const editor = Xkin.editor({
      element: document.getElementById("editor"),
      value: [
        "// Steward IDE — served entirely from the local pack origin.",
        "const hello = (name: string) => `Hello, ${name}!`;",
        "console.log(hello('world'));",
        "",
      ].join("\n"),
      language: "typescript",
    });
    // Note: deliberately NO window-global handle to the editor — capabilities are
    // granted explicitly, never discovered ambiently (design D8).
    void editor;
    status.textContent = "ready";
    // The boot ready-state signal (baseline-boot spec; updater task 6.3 listens).
    if (window.__TAURI__) {
      window.__TAURI__.core.invoke("shell_ready", {});
    }
  } catch (err) {
    status.textContent = "boot failed: " + err;
    if (window.__TAURI__) {
      window.__TAURI__.core.invoke("shell_failed", { error: String(err) });
    }
  }
});
