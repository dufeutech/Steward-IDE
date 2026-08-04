# Asset-pack system

How the app serves its frontend: one custom protocol (`pack://`, origin
`http://pack.localhost`) serves the shell and every asset pack from a single origin.
Rust never learns what an asset is — it resolves `(pack, version, path) → bytes` and
verifies hashes. Script/style tags are generated from the active pack's manifest.

```mermaid
flowchart TB
    subgraph webview [Webview — one origin]
        shell["shell (generated tags)"]
        assets["pack assets: Monaco, workers, chunks"]
    end

    subgraph protocol [Protocol adapter — thin]
        handler["pack:// handler\n+ CSP header on HTML"]
    end

    subgraph corebox [Core — pure, no I/O]
        manifest["manifest: envelope → schema → semantic"]
        resolve["resolve: normalize path → file entry"]
        verify["verify: version complete + exact"]
        tags["shell: entry tags from manifest"]
        plan["updater: plan = missing blobs only"]
    end

    subgraph store [FsStore — content-addressed]
        cas["cas/sha256/&lt;aa&gt;/&lt;hash&gt;\nimmutable blobs, hash-checked on read"]
        refs["refs/&lt;pack&gt;/&lt;semver&gt;.json\nmanifest copies = GC roots"]
        active["active/&lt;pack&gt; → semver\natomic rename flip"]
        previous["previous/&lt;pack&gt;\nretained rollback"]
        pending["pending/&lt;pack&gt;\nunconfirmed-boot marker"]
    end

    baseline["baseline pack\n(Tauri resource, in binary)\nsame manifest + hash checks"]

    updater["updater (background task)\nfetch → CAS → verify → activate+pending"]
    tuf["TufSource (tough)\nroot/timestamp/snapshot/targets\nexpiry + rollback + mix-and-match defense"]
    endpoint["static file endpoint\n(GitHub Releases)"]

    shell --> handler
    assets --> handler
    handler --> resolve
    handler --> tags
    resolve --> cas
    resolve -. "fallback: active → previous → baseline" .-> baseline
    tags --> manifest
    updater --> plan
    updater --> verify
    updater --> active
    updater --> pending
    tuf --> endpoint
    updater --> tuf
```

## Boot outcome protocol

```mermaid
sequenceDiagram
    participant U as updater
    participant S as store
    participant W as webview shell

    U->>S: activate(v2) + pending marker
    Note over W: next reload serves v2
    alt shell boots
        W->>S: shell_ready → clear pending
        Note over S: v2 confirmed; v1 stays as previous
    else shell fails (or app crashes before ready)
        W-->>S: shell_failed (or pending found at next startup)
        S->>S: rollback → v1 active again
        S-->>W: reload
    end
```

Events: `event:assets.pack_activated` / `event:assets.pack_rolled_back`
(`app/src-tauri/schemas/events.asyncapi.yaml`).

Contracts: manifest schema `app/src-tauri/schemas/pack.manifest.schema.json`;
behavior specs in `openspec/` (capabilities `asset-serving`, `pack-store`,
`pack-update`, `pack-manifest`, `baseline-boot`).
