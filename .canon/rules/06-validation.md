# Rule 6 — Validate before claiming done

**Trigger:** before considering any operation complete.

Run everything applicable to this repository. The canonical commands are in
`.canon/checks.md` — use those exact commands, and add any that are missing once discovered.

- [ ] Formatter
- [ ] Linter
- [ ] Type checker
- [ ] Unit tests
- [ ] Integration tests
- [ ] Build / compilation
- [ ] Inspect the final `git diff`
- [ ] Verify generated artifacts are correct, or correctly ignored
- [ ] Confirm architecture docs and diagrams still match the code (Rule 8)

## What a check measures

A check is evidence about the product only if it fails when the product is wrong. If its
result can turn on the machine, shell, account, or launcher that ran it, that is what it
measures — and the pass it reports is indistinguishable from a real one.

- **Establish the precondition; never inherit it.** A check that waits for its environment to
  volunteer the behaviour under test is testing the environment. Set the condition yourself,
  or report the property as unverified.
- **Building is not testing.** An artifact produced for a platform is not coverage of it, and
  a process starting is not evidence that it works.
- **A check that cannot fail has already failed.** Confirm the failure mode is reachable — a
  liveness guard satisfied by an error message is satisfied by anything.
- **When one revision disagrees across two environments, suspect the environment** until the
  product is shown to cause it. "Not caused by this change" is not a diagnosis.

> Four instances so far. Three were tests that had passed for months and broke the first time
> they ran somewhere new: a byte-transparency test that measured whether the shell colours its
> prompt, an interrupt test that measured Windows' lack of a Unix line discipline, and a
> Windows interrupt test that measured how its own launcher spawned `cargo`. The fourth was a
> coverage table that credited Windows to a matrix which builds it and tests nothing. None was
> caught by review; each was caught by running somewhere new, or by checking the claim against
> the job it cited.

The platform-specific form — what makes a platform covered rather than merely built — is a
binding contract in `openspec/specs/platform-verification/spec.md`. This section is the
general rule it specialises.

## Honesty

- A check that **cannot** run — missing tooling, no network, no database — is reported as
  **unverified**, by name. Never imply it passed.
- Never claim completion from "no git conflicts" or "the files were written". Neither is
  evidence that anything works.
- A failing check gets fixed or reported. Never buried, never softened.

State what you actually ran. "Tests pass" when you ran the formatter is a false report, and it
is the most expensive kind of error here — every later decision inherits it.
