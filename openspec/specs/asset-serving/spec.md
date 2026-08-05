# asset-serving

## Purpose

Serve the application shell and all asset-pack files from one local origin, resolving URLs to verified bytes while staying ignorant of asset content.

## Requirements

### Requirement: Single local origin for shell and packs
The system MUST serve the application shell and all asset-pack files from one local
origin, such that two files in the same pack directory are siblings under the same
URL path prefix.

#### Scenario: Relative sibling resolution
- **WHEN** a loaded asset references a sibling file by relative path (e.g. `./chunk.js` or `editor/worker.js`)
- **THEN** the reference resolves to the file at the corresponding relative location in the same pack version, with no asset-side configuration

#### Scenario: Worker spawned from a pack asset
- **WHEN** a pack asset spawns a background execution context from a URL derived from its own location
- **THEN** the spawned context loads from the same origin and same pack version as the asset that spawned it

### Requirement: URL-to-bytes resolution is content-agnostic
The resolver MUST map `(pack, version, relative path)` to stored bytes and a media type,
and MUST NOT contain logic specific to any particular asset, framework, or file name.

#### Scenario: Unknown asset type served unchanged
- **WHEN** a pack contains a file type the resolver has never seen
- **THEN** the file is served byte-identical with a media type derived from its extension, with no code change

### Requirement: Only active content is reachable
The origin MUST serve only files belonging to the currently active version of each pack
(plus the shell). Inactive versions, staged downloads, and paths outside the store MUST
NOT be reachable.

#### Scenario: Path traversal attempt
- **WHEN** a request's path attempts to escape the active pack tree (e.g. via `..` or absolute-path tricks)
- **THEN** the request is refused and no file outside the active tree is disclosed

#### Scenario: Staged but unactivated version
- **WHEN** a newer pack version is fully downloaded and verified but not yet activated
- **THEN** requests continue to resolve against the active version only

### Requirement: Remote script origins are not executable
The shell's security policy MUST forbid loading executable code from any remote origin.

#### Scenario: Remote script injection attempt
- **WHEN** markup or code attempts to load a script from a remote URL
- **THEN** the load is blocked by policy and the failure is observable in diagnostics
