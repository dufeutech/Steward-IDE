# platform-verification

## Purpose

Which platforms the repository's automated checks cover, what being covered does and does not establish, and the requirement that a check measure the product rather than the environment it happens to run in.

Verification coverage is distinct from release coverage. A platform can be checked long before anyone decides to ship it, and conflating the two turns a passing build into an implied promise of an installer.

## Requirements

### Requirement: The automated checks declare which platforms they cover

The repository MUST state the set of platforms its automated checks cover, and MUST state it
where the checks themselves are documented rather than leaving it to be inferred from
configuration.

A platform absent from that set is not a failure — it is an admission. Rule 6 requires that
what could not be run is reported as unverified, and that report is impossible while the
covered set is unstated.

#### Scenario: A reader asks which platforms are verified

- **WHEN** someone consults the documented set of checks
- **THEN** the platforms each check covers are stated there
- **AND** a platform outside that set is identifiable as uncovered rather than assumed passing

#### Scenario: A check gains or loses a platform

- **WHEN** the platforms a check runs on change
- **THEN** the documented coverage changes in the same change set

### Requirement: A platform counts as covered only when the checks execute on it

A platform MUST NOT be reported as covered on the strength of the product being *built* for
it. Coverage requires the checks to run on that platform, because a successful build
establishes only that the code compiles — not that its platform-conditional behaviour is
correct.

Where a check cannot execute on a platform, the repository MUST record it as uncovered
rather than substituting a weaker check that produces a passing result.

#### Scenario: The product compiles for a platform but nothing runs there

- **WHEN** an artifact is produced for a platform whose checks never execute on it
- **THEN** that platform is reported as uncovered
- **AND** the artifact is not treated as verified

#### Scenario: The checks execute on a platform

- **WHEN** the checks run to completion on a platform and pass
- **THEN** that platform is reported as covered to the extent of the checks that ran

### Requirement: A check measures the product, not the environment it runs in

A check MUST fail when the product is wrong and pass when the product is right, independently
of properties belonging to the machine, account, or process that happens to run it.

Where a check's outcome depends on a precondition its environment supplies by default, the
check MUST establish that precondition explicitly rather than inherit it. A check that
inherits a favourable precondition reports a pass the product has not earned, and the report
is indistinguishable from a real one.

#### Scenario: The environment supplies a precondition the product's users do not have

- **WHEN** a check would pass only because its environment omits a condition present in
  ordinary use
- **THEN** the check MUST set that condition itself before asserting
- **AND** if it cannot, the property is reported as unverified rather than checked

#### Scenario: The same check disagrees across two environments

- **WHEN** a check produces different results on the same revision of the product in two
  environments
- **THEN** the difference is attributed to the environment until the product is shown to
  cause it

### Requirement: Verification coverage does not imply release coverage

The set of platforms the checks cover and the set a release covers MUST be maintained
separately. Adding a platform to the checks MUST NOT add it to the released platform set,
and MUST NOT cause the release to offer an artifact for it.

Verification is a precondition for shipping a platform, never a substitute for the decision
to ship it.

#### Scenario: A platform is verified but not released

- **WHEN** the checks cover a platform that the release does not declare
- **THEN** the release continues to state plainly that the platform is not covered
- **AND** no artifact for it is published

#### Scenario: A platform's exclusion outlives its stated reason

- **WHEN** a platform was excluded from a release because it was unverified, and it
  subsequently becomes covered
- **THEN** the exclusion MUST be restated with a current reason or withdrawn
