# Data layout

This page documents repository files that operators may need for backup,
restore, migration, and incident work.

## Repository root

Claw metadata lives under `.claw/` in the repository root. `claw init` creates
the store directories and writes initial repository metadata.

| Path | Purpose | Operator notes |
|---|---|---|
| `.claw/objects/` | Loose COF-encoded objects, sharded by object ID prefix. | Back up with the rest of `.claw/`. |
| `.claw/packs/` | Packed object data and indexes. | `.clwpack` files are written atomically before matching `.idx` files. |
| `.claw/indices/` | Pack and lookup indexes. | Can be rebuilt only when tooling says so. |
| `.claw/cache/` | Local cache data. | Not a public storage contract. |
| `.claw/meta.db` | Reserved local index path. | Internal implementation detail. |
| `.claw/refs/heads/` | Branch refs. | Do not edit by hand. |
| `.claw/refs/intents/` | Intent refs. | Do not edit by hand. |
| `.claw/refs/changes/` | Change refs. | Do not edit by hand. |
| `.claw/refs/workstreams/` | Workstream refs. | Do not edit by hand. |
| `.claw/refs/agents/` | Agent registration refs. | Public agent metadata, not private keys. |
| `.claw/refs/policies/` | Policy refs. | Created by `claw policy`. |
| `.claw/refs/capsules/` | Capsule reverse lookup refs. | Created by `claw ship`. |
| `.claw/HEAD` | Current branch or detached object ID. | Written atomically by checkout, clone, and repository-open migration paths. |
| `.claw/reflogs/` | Append-only ref update history. | Appends are fsynced; include in backups. |
| `.claw/repo.toml` | Legacy store metadata written by `claw init`. | Kept for compatibility; written atomically. |
| `.claw/config.toml` | Runtime config schema `config_version = 1`. | Written atomically by admin migration commands. |
| `.claw/remotes.toml` | Remote aliases and auth profile names. | Written atomically by `claw remote` and `claw sync clone`. |
| `.claw/MERGE_STATE.toml` | Active merge state. | Present only during conflicted merges; written atomically. |
| `.claw/backups/` | Metadata backups from admin commands. | Snapshot files are fsynced and `manifest.json` is written atomically; replicate off-host. |
| `.claw/migrations/ledger.jsonl` | Admin migration and rollback ledger. | Appends are fsynced; keep for audit and rollback review. |
| `.claw/support/` | Support bundles. | Bundle JSON files are written atomically; review before sharing outside your org. |

## User home

Auth profiles live outside the repository:

| Path | Purpose |
|---|---|
| `~/.claw/auth.toml` | Named auth profiles and encrypted token fields. Written atomically with private file permissions. |
| `~/.claw/auth.key` | Local symmetric key used to encrypt auth tokens. Written atomically with private file permissions. |
| `~/.claw/agent-keys/` | Local Ed25519 private keys for registered agents. |

## Backup rule

Back up the full `.claw/` directory as one unit. Do not copy only `objects/` or
only `refs/`; the store needs refs, logs, config, and object data to recover.

User-home files are credential material, not repository history. Back up or
re-provision them through your credential process, and never commit them into a
repository.

## Object storage

Loose objects are COF bytes under `.claw/objects/<shard>/<object-id>`. Pack files
use `.clwpack` data files and `.idx` index files under `.claw/packs/` in the
current implementation. Treat both loose and packed object paths as opaque.

Tree entry names are stored as portable basenames, not platform-native paths.
They cannot contain path separators, control characters, Windows-reserved
characters, Windows device basenames, trailing spaces or dots, or components over
255 bytes. Tree validation also rejects duplicate basenames that would collide on
case-insensitive filesystems.

## Ref names

Claw refs are repository-relative names such as `heads/main`, `intents/<id>`,
`changes/<id>`, `agents/<name>`, and `policies/<id>`. Refs are normal files
under `.claw/refs/`, but manual edits can bypass compare-and-swap checks and
reflog recording.

Each ref path component follows the same portable filesystem constraints as tree
entry names. Ref reads, writes, deletes, prefix listing, and compare-and-swap
updates reject component names that differ only by case from an existing ref path
component, so refs such as `heads/main` and `heads/MAIN` cannot coexist or
resolve differently across Linux, macOS, and Windows.

Reflog files mirror ref names under `.claw/reflogs/` and use the same ref-name
validation before reads or appends. Reflog records are line-delimited; author and
message fields are normalized before append so embedded line breaks cannot create
synthetic reflog entries.
