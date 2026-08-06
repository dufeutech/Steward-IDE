## Why

The application pays for its asset payload twice. The embedded baseline pack is 32.3 MiB
of the ~32.4 MiB the binary carries as resources, and the first update downloads that same
payload again in full — the embedded copy is served from its own location and never counts
as content the client already holds. An installer that ships an entire editor build to
serve it for minutes, then re-fetches it byte-for-byte, is the worst of both shapes: a slow
install *and* a slow first update.

The guarantee that payload was bought — boot with no network, no prior state, or a
corrupted store — is worth keeping. What is not worth keeping is paying for it with a copy
of the production pack. A small first-party surface embedded in the binary satisfies the
same guarantee: the app always starts, always explains itself, and can always recover,
without the binary carrying any application-specific asset build.

## What Changes

- **BREAKING**: The primary asset pack is no longer embedded in the binary. A fresh install
  requires network connectivity to reach the primary application surface for the first
  time. Offline first launch reaches an interactive bootstrap surface, not the editor.
- A minimal bootstrap pack is embedded instead. It is first-party, dependency-free, and
  bounded in size — the recovery surface, never a second application.
- The binary's embedded resources drop from ~32.4 MiB to a bounded budget in the tens of
  kilobytes. The payload is downloaded once, on first run, with visible progress, instead
  of once at install and again at first update.
- A pack may declare that it has no embedded copy. Boot selects the bootstrap surface when
  the primary pack has no resolvable version, and hands off to the primary pack once one is
  active.
- First-acquisition progress, failure, and retry become observable to the shell. Today an
  update failing quietly is acceptable because the app is already usable; on first run it
  is the only thing happening and must be reportable.
- The embedded pack is regenerated from first-party source rather than fetched from an
  external origin, and is small enough to be tracked in version control rather than
  reconstructed.

Explicitly out of scope: a pack manager or "app store" surface. If one is wanted, it is an
ordinary downloaded pack. Putting it in the binary would re-introduce exactly the
application-specific weight this change removes.

## Capabilities

### New Capabilities

- `bootstrap-shell`: The embedded recovery surface — what it must do (report acquisition
  progress, report failure with a retry path, surface diagnostics when no pack can be
  served, hand off once a pack is active) and what it must never become (bounded embedded
  size, no remote fetches, no dependency on the primary pack or its toolchain). The size
  bound is a critical reliability concern: an unbounded recovery surface reintroduces the
  weight being removed and grows failure modes in the one component that must work when
  everything else is broken. Its enforcement mechanism is a build-vs-adopt decision for
  `/ai:decide`.

### Modified Capabilities

- `baseline-boot`: The offline guarantee narrows from "the shell reaches its ready state
  offline" to "the app reaches an interactive, self-explaining surface offline." The
  embedded pack is no longer required to be sufficient for the application's own ready
  state. Adds that a pack may declare no embedded copy, and that boot selects the bootstrap
  surface when the primary pack has no resolvable version. The requirement that the
  embedded pack is a normal pack, with no baseline-specific branch in the serving path, is
  unchanged and load-bearing.
- `baseline-regen`: The embedded pack's payload comes from first-party source in this
  repository, not from an external origin recorded in its manifest. Regeneration verifies
  built output against the committed manifest rather than a fetched third-party payload.
  Deterministic generation shared with publishing is unchanged.
- `pack-update`: "Failure leaves the application fully functional on its current version"
  no longer holds on first run, when there is no current version. Adds that acquisition
  progress and failure MUST be observable to the shell, and that a first acquisition which
  cannot complete leaves the app on the bootstrap surface with the reason reported, rather
  than failing silently.

## Impact

- **Boot and serving**: the fallback chain's terminal candidate; pack selection at tag
  generation; the boot ready/failed signalling path, which the bootstrap surface must also
  participate in.
- **Configuration**: the embedded-version declaration becomes optional per pack, and the
  bootstrap pack joins the pack list. Existing installs' stored state is unaffected —
  downloaded versions in the store continue to resolve and take precedence.
- **Embedded resources**: the primary pack's payload leaves the bundle; a new bootstrap
  payload enters it, tracked in version control.
- **Regeneration and publishing tooling**: the embedded-pack path changes source from
  external fetch to local build; the publishing pipeline for the primary pack is otherwise
  untouched, and the trust anchor, its ceremony, and the update transport are entirely
  untouched.
- **First-run dependency**: the update endpoint becomes required for a new install to reach
  the primary surface. Existing installs are unaffected. This is a deliberate trade of
  install-time independence for install size, and it makes endpoint availability a
  user-visible concern at first launch rather than only at update time.
- **Documentation**: the asset-pack architecture diagram and the offline-boot claims in
  operator-facing docs.
