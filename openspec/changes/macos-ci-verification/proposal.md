## Why

macOS has never been compiled or tested in this project. That is the stated reason the
release set excludes it (`binary-release-pipeline` D6: "excluded because it has never run"),
and it is the only platform where nobody can say whether the code even builds — the Windows
and Unix arms of the terminal code are each exercised somewhere, macOS is exercised nowhere.

Nothing made this expensive except the absence of a host. The source repository is now
public, so hosted macOS runners cost nothing, and the checks that already run on Linux can
run on macOS without a machine being bought, borrowed, or maintained.

This change removes "it has never run" as a fact. It does **not** ship macOS.

## What Changes

- The automated checks gain macOS coverage: the application compiles and the Rust test
  suite executes on a hosted macOS runner, on the same trigger as the existing checks.
- The repository states which platforms its checks cover, so "covered" stops being folklore
  read off a workflow file.
- The checks' documented command set records what macOS coverage does and does not
  establish.

Explicitly **not** in scope, and stated here so a later reader does not infer it:

- macOS is **not** added to the release platform set. No installer is bundled, uploaded, or
  attested, and `app-release`'s platform requirement is untouched.
- No claim is made about **Gatekeeper, quarantine, notarization, or signing**. See the trap
  below — a naive check here would produce a confident, false answer.
- No macOS host is claimed for the by-hand verification that the Linux and Windows artifacts
  received.

**The trap this change must not fall into.** A CI job that downloads a release artifact with
an ordinary command-line fetch receives no quarantine attribute, because that attribute is
set by the downloading application and command-line tools do not set it. Gatekeeper's
verdict is conditioned on it. Such a job would launch the application successfully and
report a pass that no real user experiences — measuring the runner's environment rather than
the product. This repository has caught that same failure three times already, in two
sessions. The scope above is drawn to keep it out rather than to catch it later.

## Capabilities

### New Capabilities

- `platform-verification`: which platforms the repository's automated checks cover, what
  being covered does and does not establish, and the requirement that a check measure the
  product rather than the environment it happens to run in.

### Modified Capabilities

None. `app-release` governs the platforms a *release* covers; this change adds no platform
to that set and alters none of its requirements. The two are deliberately separate: a
platform can be verified long before it is shipped, and conflating them is what would turn a
green build into an implied promise of an installer.

## Impact

- **Checks workflow** — a macOS job alongside the existing Linux ones.
- **`.canon/checks.md`** — the platform coverage of existing rows becomes explicit; Rule 6
  requires that what was not run is reported as unverified, which is unanswerable today.
- **Possible source changes, extent unknown.** Nothing here has ever been compiled for
  macOS. Platform-conditional code — the PTY and console-control adapters most of all — may
  not build, and the test suite may contain assertions that hold on Linux and Windows for
  reasons that do not carry over. Discovering this is the point of the change; the size of
  it cannot be known before the first run.
- **`DECISIONS.md` / D6** — the recorded reason for excluding macOS from the release set is
  "it has never run". Once it has, that reason no longer holds and the exclusion needs a
  current one (signing and notarization cost), or it needs revisiting.

## Open question, deliberately not decided here

Three tests in two sessions have now been caught measuring their environment rather than the
product. This change encodes that property for platform coverage specifically. Whether the
general form belongs in `.canon/rules/` is a judgement about how we work, not about this
change's behaviour, and it is left open rather than settled as a side effect.
