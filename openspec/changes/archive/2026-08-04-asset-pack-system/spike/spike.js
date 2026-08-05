const report = {
  origin: location.origin,
  errors: [],
  csp_violations: [],
  remote_resources: [],
  local_resource_count: 0,
  editor_rendered: false,
  value_roundtrip: false,
  models_created: [],
};
window.addEventListener("error", (e) =>
  report.errors.push(String((e && e.message) || e)),
);
window.addEventListener("unhandledrejection", (e) =>
  report.errors.push("rejection: " + String(e.reason)),
);
document.addEventListener("securitypolicyviolation", (e) =>
  report.csp_violations.push(e.violatedDirective + " ← " + e.blockedURI),
);

// CSP canaries: if the policy is enforced, both must be blocked.
try {
  new Function("return 1")();
  report.errors.push("CANARY: eval ALLOWED — CSP not enforced");
} catch (e) {
  report.csp_violations.push("canary: eval blocked (expected)");
}
const canaryImg = new Image();
canaryImg.src = "https://example.com/csp-canary.png";

try {
  const ed = Xkin.editor({
    element: document.getElementById("ed"),
    value: "const x: number = 1;\nconsole.log(x);",
    language: "typescript",
  });
  report.value_roundtrip = ed.getValue().includes("const x");
  // Force the other language workers to spawn: create each model AND attach
  // it to the editor so its language service activates.
  const tsModel = ed.getModel();
  const cycle = [
    ["/spike.css", "body { color: red; }", "css"],
    ["/spike.html", "<div>spike</div>", "html"],
    ["/spike.json", '{"spike": true}', "json"],
  ];
  let delay = 1000;
  for (const [path, content, lang] of cycle) {
    try {
      const m = Xkin.create_model(path, content);
      report.models_created.push(path);
      setTimeout(() => {
        try {
          Xkin.set_language(m, lang);
          ed.setModel(m);
        } catch (err) {
          report.errors.push("setModel " + path + ": " + String(err));
        }
      }, delay);
      delay += 1500;
    } catch (err) {
      report.errors.push("create_model " + path + ": " + String(err));
    }
  }
  setTimeout(() => ed.setModel(tsModel), delay);
} catch (err) {
  report.errors.push("boot: " + String(err));
}

// Give workers and lazy chunks time to load, then report.
setTimeout(() => {
  const res = performance.getEntriesByType("resource").map((r) => r.name);
  report.local_resource_count = res.filter((u) =>
    u.startsWith(location.origin),
  ).length;
  report.remote_resources = res.filter(
    (u) => !u.startsWith(location.origin),
  );
  report.editor_rendered = !!document.querySelector(".monaco-editor");
  document.getElementById("status").textContent = JSON.stringify(
    report,
    null,
    2,
  );
  window.__TAURI__.core.invoke("spike_report", {
    report: JSON.stringify(report),
  });
}, 8000);
