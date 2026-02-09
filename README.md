# Claw

**Intent-native, agent-native version control.**

Claw is a version control system built for a world where AI agents write code alongside humans. It tracks *why* changes were made — not just *what* changed — and provides cryptographic proof of who did the work and what checks they passed.

```
claw init
claw intent create --title "Add dark mode" --goal "Support light/dark theme toggling"
claw change create --intent <intent-id>
# ... write code ...
claw snapshot -m "Initial dark mode implementation"
claw ship --intent <intent-id>
```

## Why Claw exists

Git was designed in 2005 for human kernel developers emailing patches. It does its job well: it tracks who changed what lines, when, and (if the commit message is good) why.

But the world is changing. AI agents are writing significant amounts of code. And Git's model has gaps that matter in this new world:

**Git can't prove what checks passed.** Alice says "I ran the tests" in her commit message. Did she? Git doesn't know. CI runs externally, and the results live in GitHub Actions, not in the repo. If GitHub goes down, that proof is gone.

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

### Capsules: agent provenance with cryptographic proof

When an agent makes a change, it produces a **Capsule** — a signed envelope containing:

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

The evidence (test results, lint outcomes, security scan results) is stored as signed objects in the repo. You can verify them offline, years later, without any external service.

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

## Architecture

Claw is written in Rust and organized as a workspace of focused crates:

```
crates/
├── claw-core       Core types and COF (Claw Object Format) codec
├── claw-store      Content-addressed object store with refs, HEAD, reflog
├── claw-patch      Pluggable codec-based diff/apply/merge engine
├── claw-merge      Three-way merge with conflict resolution
├── claw-crypto     Ed25519 signing, verification, capsule construction
├── claw-policy     Policy evaluation and visibility enforcement
├── claw-sync       gRPC sync protocol with partial clone
├── claw-git        Bidirectional Git export/import
└── claw-cli        The `claw` binary

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

Claw uses **gRPC with HTTP/2 streaming** for network operations:

```protobuf
service SyncService {
  rpc Hello(HelloRequest) returns (HelloResponse);
  rpc AdvertiseRefs(AdvertiseRefsRequest) returns (AdvertiseRefsResponse);
  rpc FetchObjects(FetchObjectsRequest) returns (stream ObjectChunk);
  rpc PushObjects(stream ObjectChunk) returns (PushObjectsResponse);
  rpc UpdateRefs(UpdateRefsRequest) returns (UpdateRefsResponse);
}
```

Partial clone filters let you fetch selectively:

- **Intent IDs** — fetch only work related to a specific goal
- **Path prefixes** — fetch only `src/frontend/`
- **Time ranges** — fetch only recent work
- **Codec types** — fetch only JSON files
- **Capsule visibility** — respect public/private/restricted access
- **Byte budget / depth limit** — resource-constrained fetching

### Daemon

`claw daemon` (or `claw serve`) runs a long-lived gRPC server exposing services for intents, changes, capsules, workstreams, events, and sync. Agents connect programmatically — create intents, submit changes, stream events in real-time. Git has no equivalent.

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
claw snapshot -m "msg"       Record the working tree atomically
claw ship --intent <id>      Finalize a change and produce a capsule
claw integrate               Merge changes (three-way, codec-aware)
claw branch <subcommand>     List, create, or delete branches
claw checkout <branch>       Switch branches or restore working tree
claw log                     Show revision history
claw diff                    Show changes between trees
claw status                  Show working tree status
claw show <object-id>        Inspect any object
claw resolve <subcommand>    Manage merge conflicts
claw agent <subcommand>      Register and manage agent identities
claw remote <subcommand>     Manage remote repositories
claw sync <remote>           Sync with a remote repository
claw daemon                  Run the gRPC sync server
claw patch <subcommand>      Create and apply patches directly
claw git-export              Export to Git format
```

**No staging area** — by design. `claw snapshot` captures everything atomically. This is a deliberate simplification for agent workflows where partial staging adds complexity without value.

## What Claw does that Git can't

| Capability | Git | Claw |
|---|---|---|
| Track *why* a change was made (structured) | Freeform commit message | Intent objects with goals, constraints, acceptance tests |
| Prove what checks passed | External CI; not in the repo | Evidence in capsules, signed and stored in-repo |
| Distinguish human from AI agent | Freeform author string | Registered agent identities with Ed25519 keys |
| Enforce policies in the repo | GitHub/GitLab settings (external) | Policy objects versioned alongside code |
| Codec-aware merging | Line-based diff only | Pluggable codecs (JSON tree diff, etc.) |
| Partial clone by intent/time/codec | Treeless/blobless only | Filter by intent, path, time, codec, visibility, byte budget |
| Agent-native daemon | None | gRPC server for programmatic agent access |
| Patch commutation | N/A | Darcs-style independent patch reordering |
| Capsule encryption | N/A | XChaCha20-Poly1305 encrypted private metadata |

### Where Git is honestly fine

- **Human authorship** — Git's author/committer model works well for human developers.
- **Commit messages as "why"** — perfectly adequate for most human-only projects.
- **GPG signing** — proves a human signed a commit. Claw's capsule signatures extend the same idea to agents and evidence.

**The thesis:** If your project is 100% human-written, Git's provenance model is probably sufficient. If agents are submitting changes autonomously, Git has no way to enforce "only integrate if the agent proved it ran the test suite" without external infrastructure. Claw bakes it in.

## Building from source

Requires [Rust](https://rustup.rs/) (stable) and [protoc](https://grpc.io/docs/protoc-installation/) (for Protocol Buffer compilation).

```bash
git clone https://github.com/<your-org>/claw.git
cd claw
cargo build --release
```

The binary is at `target/release/claw`.

Run tests:

```bash
cargo test --workspace
```

## Project status

Claw is **v0.1.0** — early and evolving. The core object model, storage engine, patch system, cryptographic layer, policy engine, sync protocol, and CLI are implemented. Contributions and feedback are welcome.

## License

MIT
