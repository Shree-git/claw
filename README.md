# Claw VCS

[![CI](https://github.com/Shree-git/claw-vcs/actions/workflows/ci.yml/badge.svg)](https://github.com/Shree-git/claw-vcs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Security policy](https://img.shields.io/badge/security-policy-informational.svg)](SECURITY.md)

**Intent-native, agent-native version control.**

> Status: v0.1 experimental. Claw VCS is suitable for local exploration, demos, and design feedback.
> It is not yet recommended as the sole source of truth for production repositories.

Claw VCS is a version control system built for a world where AI agents write code alongside humans. It tracks *why* changes were made, not just *what* changed, and stores signed claims about who did the work and what checks they ran.

```
claw init
claw intent create --title "Add dark mode" --goal "Support light/dark theme toggling"
claw change create --intent <intent-id>
# ... write code ...
claw snapshot --change <change-id> -m "Initial dark mode implementation"
claw ship --intent <intent-id> --revision-ref heads/main --evidence test=pass --evidence lint=pass
```

## Why Claw VCS exists

Git remains excellent for human-authored source history. Claw VCS explores a complementary model for agent-written code where intent, evidence, and policy are first-class repository objects.

AI agents are writing significant amounts of code. Git's model has gaps that matter in this new world:

**Git does not store structured check evidence.** Alice says "I ran the tests" in her commit message. Git records the message, not a signed, queryable claim about the command, runner, revision, and result. CI runs externally, and the results often live in GitHub Actions rather than in the repository.

**Git can't distinguish human Alice from bot Alice.** If an AI agent commits as alice@example.com, Git can't tell. There's no structured way to say "this was written by Claude 3.5 Sonnet, using toolchain X, in environment Y." Git's author field is a freeform string.

**Git's "why" is a freeform string.** There's no way to query "show me all changes related to the dark mode feature" without grepping commit messages. The link between intent and implementation is informal.

**Git's policies are external.** Branch protection, required reviews, required CI checks — all live in GitHub/GitLab settings, not in the repo. Fork the repo and those rules disappear.

Claw addresses each of these with first-class primitives.

## Core concepts

### Intent → Change → Revision

Git tracks what changed. Claw tracks *why*.

```
Intent ("add dark mode")          ← the goal, with constraints and acceptance tests
└── Change                        ← an implementation attempt
    └── Revision                  ← a snapshot of code (like a commit)
```

Intents are versioned objects in the repo — not external issues in Jira. They have structured fields: `goal`, `constraints`, `acceptance_tests`, `status`. Changes link to intents by ID. You can programmatically ask "what revisions addressed intent X, and did they all pass the acceptance tests?"

### Capsules: signed agent provenance claims

When an agent makes a change, it produces a **Capsule**, a signed envelope containing:

```
Capsule {
    revision_id: clw_...,
    public_fields: {
        agent_id: "claude-3.5-sonnet",
        agent_version: "20240620",
        toolchain_digest: "sha256:a1b2c3...",
        env_fingerprint: "linux-x86_64-rust-1.79",
        evidence: [
            { name: "test",          status: "pass", duration_ms: 4200 },
            { name: "lint",          status: "pass", duration_ms: 800  },
            { name: "security-scan", status: "pass", duration_ms: 1500 },
        ]
    },
    signatures: [{ signer_id: "agent-key-01", signature: <Ed25519> }],
    encrypted_private: <optional XChaCha20-Poly1305 blob>,
}
```

The evidence (test results, lint outcomes, security scan results) is stored in signed capsules in the repo. Claw can verify that a signed capsule claims specific evidence over a specific revision, produced by a specific key. Whether that evidence is trustworthy depends on key management, runner integrity, and policy configuration.

### Policies as versioned objects

```
Policy {
    required_checks:  ["test", "lint", "security-scan"],
    sensitive_paths:  ["secrets/", "admin/"],
    quarantine_lane:  true,
    min_trust_score:  "0.8",
    visibility:       Public,
}
```

Policies are stored in the repo and travel with it. Integration can be gated: "this revision's capsule must have evidence that all required checks passed." Fork the repo and the policies come with it.

Current enforcement highlights:

- `required_reviewers` are matched against verified capsule signer identities (agent IDs or key IDs).
- `sensitive_paths` require encrypted capsule private fields when touched.
- `quarantine_lane` blocks automated integration when sensitive paths are touched.
- `min_trust_score` supports `0.0-1.0` or percent formats (for example `0.85`, `85%`) and is evaluated from capsule evidence pass ratio.

## Architecture

Claw is written in Rust and organized as a workspace of focused crates:

```
crates/
├── claw-core       Core types and COF codec (package `claw-vcs-core`)
├── claw-store      Object store, refs, HEAD, reflog (package `claw-vcs-store`)
├── claw-patch      Codec-based diff/apply/merge engine (package `claw-vcs-patch`)
├── claw-merge      Three-way merge with conflicts (package `claw-vcs-merge`)
├── claw-crypto     Signing, verification, capsules (package `claw-vcs-crypto`)
├── claw-policy     Policy and visibility checks (package `claw-vcs-policy`)
├── claw-sync       gRPC sync with daemon fetch filters (package `claw-vcs-sync`)
├── claw-git        Git import/export (package `claw-vcs-git`)
└── claw            The `claw-vcs` Cargo package, publishing the `claw` binary

proto/              Protocol Buffer definitions for all gRPC services
```

### Object model

Claw has 12 first-class object types, each with a unique type tag:

| Type | Tag | Purpose |
|------|-----|---------|
| **Blob** | `0x01` | File content with optional media type |
| **Tree** | `0x02` | Directory structure (name, mode, object ID per entry) |
| **Patch** | `0x03` | Codec-specific diff operations between two states |
| **Revision** | `0x04` | A point in history (parents, patches, author, timestamp) |
| **Snapshot** | `0x05` | Atomic capture of the full working tree |
| **Intent** | `0x06` | A goal with constraints and acceptance tests |
| **Change** | `0x07` | An implementation attempt linked to an intent |
| **Conflict** | `0x08` | Merge conflict state (base/left/right) |
| **Capsule** | `0x09` | Signed agent provenance envelope with evidence |
| **Policy** | `0x0A` | Versioned enforcement rules |
| **Workstream** | `0x0B` | Ordered stack of related changes |
| **RefLog** | `0x0C` | Append-only reference change history |

Every object is serialized with Protocol Buffers and wrapped in the **COF** (Claw Object Format):

```
┌──────────┬─────────┬──────────┬───────┬─────────────┬──────────────────────┬─────────┬────────┐
│ Magic    │ Version │ TypeTag  │ Flags │ Compression  │ Uncompressed Length  │ Payload │ CRC32  │
│ "CLW1"   │  0x01   │  u8     │ u8    │ None | Zstd  │ uvarint              │ ...     │ 4B LE  │
└──────────┴─────────┴──────────┴───────┴─────────────┴──────────────────────┴─────────┴────────┘
```

Objects are content-addressed using **BLAKE3** with domain separation (`"claw\0" || type_tag || version || payload`), so different object types with identical content can never collide. IDs are displayed as `clw_` + lowercase Base32 (e.g., `clw_ab3fg7kl...`).

### Codec-aware patching

The patch system is pluggable. Each codec implements five operations:

```rust
trait Codec {
    fn diff(&self, old: &[u8], new: &[u8]) -> Vec<PatchOp>;
    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Vec<u8>;
    fn invert(&self, ops: &[PatchOp]) -> Vec<PatchOp>;
    fn commute(&self, left: &[PatchOp], right: &[PatchOp]) -> (Vec<PatchOp>, Vec<PatchOp>);
    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Vec<u8>;
}
```

Built-in codecs:

| Codec | ID | File types | Strategy |
|-------|----|------------|----------|
| **Text/Line** | `text/line` | `.txt`, `.md`, `.rs`, `.py`, ... | Line-based diff (similar to `diff`) |
| **JSON/Tree** | `json/tree` | `.json` | Structural tree diff (keys, not lines) |
| **Binary** | `binary` | Everything else | Full-blob replacement |

The `commute` operation enables Darcs-style patch reordering — if two patches touch independent parts of a file, they can be applied in either order without conflict.

The architecture supports adding codecs for YAML, TOML, SQL migrations, Protobuf schemas — anything where structural understanding beats line-by-line diff.

### Sync protocol

Claw uses **gRPC with HTTP/2 streaming** for network operations, with an optional
HTTP transport adapter for planned hosted remotes:

```protobuf
service SyncService {
  rpc Hello(HelloRequest) returns (HelloResponse);
  rpc AdvertiseRefs(AdvertiseRefsRequest) returns (AdvertiseRefsResponse);
  rpc FetchObjects(FetchObjectsRequest) returns (stream ObjectChunk);
  rpc PushObjects(stream ObjectChunk) returns (PushObjectsResponse);
  rpc UpdateRefs(UpdateRefsRequest) returns (UpdateRefsResponse);
}
```

At the daemon protocol layer, `FetchObjects` accepts filters for selective object
fetches:

- **Intent IDs** — fetch only work related to a specific goal
- **Path prefixes** — fetch only `src/frontend/`
- **Time ranges** — fetch only recent work
- **Codec types** — fetch only JSON files
- **Capsule visibility** — respect public/private/encrypted-metadata-required policy modes
- **Byte budget / depth limit** — resource-constrained fetching

Current CLI limitation: `claw sync clone` still performs a full clone and does
not expose these filters.

### Daemon

`claw daemon` (or `claw serve`) runs a long-lived gRPC server exposing services for intents, changes, capsules, workstreams, events, and sync. Agents connect programmatically — create intents, submit changes, stream events in real-time. Git has no equivalent.

For production profile runs, non-local daemon binds require authentication and TLS by default. Daemon auth can be configured with `--auth-token` (explicit bearer token) or `--auth-profile` (reuse token from `claw auth` profile).

### Cryptography

| Primitive | Algorithm | Purpose |
|-----------|-----------|---------|
| Hashing | BLAKE3 | Content addressing (faster + more secure than SHA-1) |
| Signing | Ed25519 | Capsule signatures, agent identity |
| Encryption | XChaCha20-Poly1305 | Optional capsule private data |
| Integrity | CRC32 | COF format corruption detection (independent of hash) |
| Compression | Zstd | Object storage and transport (faster + better ratio than zlib) |

## CLI reference

```
claw init                    Initialize a new repository
claw intent <subcommand>     Create and manage intents
claw change <subcommand>     Create and manage changes
claw policy <subcommand>     Create and manage integration policies
claw snapshot -m "msg"       Record the working tree atomically
claw ship --intent <id>      Finalize a revision and produce a capsule (use --revision-ref for branches)
claw integrate --right <ref> Merge changes (three-way, codec-aware)
claw branch <subcommand>     List, create, or delete branches
claw checkout <branch>       Switch branches or restore working tree
claw log                     Show revision history
claw diff                    Show changes between trees
claw status                  Show working tree status
claw show <object-id>        Inspect any object
claw resolve <subcommand>    Manage merge conflicts
claw agent <subcommand>      Register and manage agent identities
claw remote <subcommand>     Manage remote repositories
claw auth <subcommand>       Manage auth profiles and tokens for explicit remote URLs; hosted remotes are planned
claw sync <remote>           Pull from a remote (shorthand)
claw sync <subcommand>       Push, pull, or clone
claw daemon                  Run the gRPC sync server
claw patch <subcommand>      Create and apply patches directly
claw git-export              Export to Git format (supports --all-heads, --git-notes)
claw git-import              Import from Git format (supports --all-branches, --read-notes)
claw git-roundtrip           Verify claw -> git -> claw integrity for a ref
```

**No staging area** — by design. `claw snapshot` captures everything atomically. This is a deliberate simplification for agent workflows where partial staging adds complexity without value.

## What Claw VCS adds alongside Git

| Capability | Git | Claw |
|---|---|---|
| Track *why* a change was made (structured) | Freeform commit message | Intent objects with goals, constraints, acceptance tests |
| Record signed check claims | External CI; not in the repo | Evidence in capsules, signed and stored in-repo |
| Distinguish human from AI agent | Freeform author string | Registered agent identities with Ed25519 keys |
| Enforce policies in the repo | GitHub/GitLab settings (external) | Policy objects versioned alongside code |
| Codec-aware merging | Line-based diff only | Pluggable codecs (JSON tree diff, etc.) |
| Daemon fetch filters by intent/time/codec | Treeless/blobless only | Daemon object fetch can filter by intent, path, time, codec, visibility, and byte budget; CLI clone currently fetches all refs/objects |
| Agent-native daemon | None | gRPC server for programmatic agent access |
| Patch commutation | N/A | Darcs-style independent patch reordering |
| Capsule encryption | N/A | XChaCha20-Poly1305 encrypted private metadata |

### Where Git is honestly fine

- **Human authorship** — Git's author/committer model works well for human developers.
- **Commit messages as "why"** — perfectly adequate for most human-only projects.
- **GPG/SSH signing** — proves a configured key signed a commit. Claw's capsule signatures extend the same idea to agent keys and evidence claims.

**The thesis:** If your project is 100% human-written, Git's provenance model is probably sufficient. If agents are submitting changes autonomously, Git has no built-in repository object for enforcing "only integrate if a trusted key signed acceptable test evidence for this exact revision." Claw makes that evidence and policy part of the repo.

## What Claw VCS is not

- Not a Git replacement for every project.
- Not a CI system.
- Not a code review platform.
- Not proof that code is correct.
- Not proof that tests are sufficient.
- Not proof that an AI agent behaved safely.
- Not full supply-chain security by itself.

## Install

Tagged releases publish prebuilt binaries for macOS, Linux, and Windows in GitHub Releases. The verification blocks below require a release built from this hardened tree; the existing `v0.1.0` release from 2026-05-01 predates `claw doctor` and should be treated as historical for launch verification.

For the current working tree, the source install path has been smoke-tested with `cargo install --path crates/claw --locked`. For release channels, check [install verification](docs/operations/install-verification-log.md) and only mark a channel launch-ready after its current public artifact passes the matching clean-environment smoke test.

### Current source install

Use this path until a launch-hardening release is published and verified:

```bash
git clone https://github.com/shree-git/claw-vcs.git
cd claw-vcs
cargo install --path crates/claw --locked
claw --version
claw doctor
```

### Release channels

The release-channel commands below are for the next verified launch-hardening
release. Until that tag is recorded in the install verification log, use the
current source install above. Do not treat `/latest`, Homebrew, MSI, or installer output as
launch-ready until the release notes and
[install verification log](docs/operations/install-verification-log.md) record a
passing current-tag verification. In the examples below, replace `<launch-tag>`
with that verified tag.

### macOS

**Homebrew**

```bash
brew install shree-git/tap/claw
```

Or, if you prefer:

```bash
brew tap shree-git/tap
brew install shree-git/tap/claw
```

Verify:

```bash
claw --version
claw doctor
mkdir -p /tmp/claw-demo && cd /tmp/claw-demo
claw init
claw status
```

**Manual download**

Grab the verified macOS archive for `<launch-tag>` from [GitHub Releases](https://github.com/shree-git/claw-vcs/releases),
verify it with [release verification](docs/security/verifying-releases.md), extract it, and place `claw` somewhere on your `PATH` (for example `~/.local/bin`).

**Installer script**

Prefer downloading and inspecting the installer before running it:

```bash
curl --proto '=https' --tlsv1.2 -LsSfO https://github.com/shree-git/claw-vcs/releases/download/<launch-tag>/claw-installer.sh
sh ./claw-installer.sh
```

Pipe-to-shell convenience form, after you have verified the release:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/shree-git/claw-vcs/releases/download/<launch-tag>/claw-installer.sh | sh
```

Install to a custom location:

```bash
CLAW_HOME="$HOME/.claw" sh ./claw-installer.sh
```

Verify:

```bash
claw --version
claw doctor
mkdir -p /tmp/claw-demo && cd /tmp/claw-demo
claw init
claw status
```

### Linux

**Manual download**

Grab the verified Linux archive for `<launch-tag>` from [GitHub Releases](https://github.com/shree-git/claw-vcs/releases),
verify it with [release verification](docs/security/verifying-releases.md), extract it, and place `claw` somewhere on your `PATH` (for example `~/.local/bin`).

**Installer script**

Prefer downloading and inspecting the installer before running it:

```bash
curl --proto '=https' --tlsv1.2 -LsSfO https://github.com/shree-git/claw-vcs/releases/download/<launch-tag>/claw-installer.sh
sh ./claw-installer.sh
```

Pipe-to-shell convenience form, after you have verified the release:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/shree-git/claw-vcs/releases/download/<launch-tag>/claw-installer.sh | sh
```

Install to a custom location:

```bash
CLAW_HOME="$HOME/.claw" sh ./claw-installer.sh
```

Verify:

```bash
claw --version
claw doctor
mkdir -p /tmp/claw-demo && cd /tmp/claw-demo
claw init
claw status
```

Notes:

- On NixOS (or other unusual environments), prefer manual install (download a release archive and place `claw` somewhere on your `PATH`) or build from source.

### Windows

**WinGet (planned)**

WinGet support is planned, but no WinGet install command is launch-ready until a manifest is accepted in the upstream Microsoft repository. Until release notes and the install verification log mark a Windows channel verified for the current tag, use the current source install or a manually verified GitHub release artifact.

**MSI**

Download the verified `.msi` for `<launch-tag>` from GitHub Releases and run it. The installer adds `claw` to `PATH`.

**PowerShell installer (no MSI)**

Prefer downloading and inspecting the installer before running it:

```powershell
iwr -useb https://github.com/shree-git/claw-vcs/releases/download/<launch-tag>/claw-installer.ps1 -OutFile claw-installer.ps1
powershell -ExecutionPolicy Bypass -File .\claw-installer.ps1
```

Pipe-to-`iex` convenience form, after you have verified the release:

```powershell
iwr -useb https://github.com/shree-git/claw-vcs/releases/download/<launch-tag>/claw-installer.ps1 | iex
```

Verify:

```powershell
claw --version
claw doctor
mkdir $env:TEMP\claw-demo -Force
cd $env:TEMP\claw-demo
claw init
claw status
```

### Build from source (any OS)

Requires [Rust](https://rustup.rs/) (stable). Claw uses a vendored `protoc` by default, so you
generally **don't** need to install Protocol Buffers tooling. If you want to force a specific
`protoc`, set `PROTOC=/path/to/protoc` before building.

```bash
git clone https://github.com/shree-git/claw-vcs.git
cd claw-vcs
cargo build --release -p claw-vcs
CLAW_BIN="$(pwd)/target/release/claw"
"$CLAW_BIN" --version
"$CLAW_BIN" doctor
mkdir /tmp/claw-demo
cd /tmp/claw-demo
"$CLAW_BIN" init
"$CLAW_BIN" status
```

Run tests:

```bash
cargo test --workspace
```

### Rust users (cargo)

If you already have Rust installed, you can install directly with cargo:

```bash
cargo install --git https://github.com/shree-git/claw-vcs.git --package claw-vcs --locked
claw --version
claw doctor
mkdir /tmp/claw-demo
cd /tmp/claw-demo
claw init
claw status
```

For a release-specific source install, add the tag:

```bash
cargo install --git https://github.com/shree-git/claw-vcs.git --tag vX.Y.Z --package claw-vcs --locked
```

## Verify releases

Before trusting a downloaded artifact, verify checksums, signatures, and attestations when release assets provide them. See [release verification](docs/security/verifying-releases.md).

## Uninstall

See [uninstall instructions](docs/operations/uninstall.md) for Homebrew, MSI, manual installs, config, auth profiles, and local repo state.

## Documentation

- Full operator docs: [docs/README.md](docs/README.md)
- Quickstart: [docs/getting-started/quickstart.md](docs/getting-started/quickstart.md)
- Concepts: [docs/concepts/index.md](docs/concepts/index.md)
- Workflows: [docs/workflows/index.md](docs/workflows/index.md)
- Agent integration: [docs/agents/index.md](docs/agents/index.md)
- Migration from Git: [docs/migration/index.md](docs/migration/index.md)
- Persona guide: [docs/persona/index.md](docs/persona/index.md)
- CLI reference: [docs/cli/README.md](docs/cli/README.md)
- Support: [SUPPORT.md](SUPPORT.md)
- Roadmap: [ROADMAP.md](ROADMAP.md)
- Contributing: [CONTRIBUTING.md](CONTRIBUTING.md)
- Production readiness checklist: [docs/reference/production-readiness-checklist.md](docs/reference/production-readiness-checklist.md)
- Basic demo: [scripts/demo.sh](scripts/demo.sh) and [examples/basic-demo/README.md](examples/basic-demo/README.md)
- Backup/restore demo: [examples/backup-restore/README.md](examples/backup-restore/README.md)
- Demo media: [examples/demo-media/README.md](examples/demo-media/README.md)
- Production install: [docs/operations/production-install.md](docs/operations/production-install.md)
- Upgrade and rollback: [docs/operations/upgrade-and-rollback.md](docs/operations/upgrade-and-rollback.md)
- Disaster recovery: [docs/operations/disaster-recovery.md](docs/operations/disaster-recovery.md)
- Troubleshooting: [docs/operations/troubleshooting.md](docs/operations/troubleshooting.md)
- Runbooks index: [docs/runbooks/README.md](docs/runbooks/README.md)

## Project status

Claw VCS is **v0.1.2-beta.1 beta candidate**. The repository includes release, operator, rollback, and production preflight tooling, but public release channels must be verified per release before they are treated as live. Keep Git or another proven system as the source of truth while evaluating Claw VCS.

## License

MIT
