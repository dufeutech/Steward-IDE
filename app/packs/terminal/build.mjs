// Build the terminal pack's payload (design D7).
//
// This is the repository's first frontend build step, and it is scoped to this directory
// alone: `shell/` stays build-free and the Rust build is untouched, so a broken toolchain
// here can never stop the application from building.
//
// Output is an IIFE, not a module. The shell injects pack entry points as plain
// `<script src>` tags in manifest order (`core::shell::entry_tags`), and a classic script
// is what those load.

import { build } from "esbuild";
import { rm, mkdir, readdir, stat, readFile } from "node:fs/promises";
import { join } from "node:path";

const OUT = "dist";

await rm(OUT, { recursive: true, force: true });
await mkdir(OUT, { recursive: true });

const result = await build({
  entryPoints: { terminal: "src/terminal.js" },
  bundle: true,
  outdir: OUT,
  format: "iife",
  // The webview is a current WebView2/WKWebView/WebKitGTK; there is no legacy browser to
  // support, so nothing is down-levelled that does not need to be.
  target: ["chrome110", "safari16"],
  minify: true,
  sourcemap: false,
  legalComments: "none",
  // Everything must be inside the bundle: `default-src 'self'` blocks every other origin,
  // so an external reference is not a slow path, it is a broken one.
  external: [],
  loader: { ".woff": "dataurl", ".woff2": "dataurl", ".ttf": "dataurl" },
  logLevel: "info",
  metafile: true,
});

// A remote reference in the output would fail closed under the CSP at runtime rather than
// here, which is the worst place to find out. Cheap to assert, so assert it.
//
// XML namespace URIs are exempt and must be: `createElementNS` and SVG both take
// `http://www.w3.org/...` as an *identifier* that is compared as a string and never
// fetched. Flagging them would make the check cry wolf on every build, which is how a
// check stops being read.
const NAMESPACE_URI = /^https?:\/\/www\.w3\.org\//i;
const outputs = await readdir(OUT);
const offenders = [];
for (const name of outputs) {
  const path = join(OUT, name);
  if (!(await stat(path)).isFile()) continue;
  const text = await readFile(path, "utf8");
  for (const match of text.matchAll(/(https?:)?\/\/[a-z0-9.-]+\.[a-z]{2,}\//gi)) {
    if (NAMESPACE_URI.test(match[0])) continue;
    offenders.push(`${name}: ${match[0]}`);
  }
}
if (offenders.length) {
  console.error(
    "\nRemote origins in the built payload — these would be blocked by `default-src 'self'`:",
  );
  for (const o of offenders) console.error("  " + o);
  process.exit(1);
}

const bytes = Object.entries(result.metafile.outputs)
  .map(([name, o]) => `  ${name}  ${(o.bytes / 1024).toFixed(1)} KiB`)
  .join("\n");
console.log(`\nterminal pack payload:\n${bytes}`);
