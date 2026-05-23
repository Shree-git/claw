# Object and Protocol Stability Tiers

This page marks repository object fields and protocol surfaces as stable,
experimental, reserved, or internal for the `v0.1.x` line.

## Tier Meanings

| Tier | Meaning |
|---|---|
| stable | The field is part of the documented machine contract and should only change with deliberate migration guidance. |
| experimental | The field is public enough to inspect, but semantics may change before v1.0. |
| reserved | The field name, tag, or concept is held for compatibility and must not be reused for another meaning. |
| internal | Implementation detail; do not build external automation on it. |

## Core Object Fields

| Object | Field | Tier | Notes |
|---|---|---|---|
| `Blob` | `data` | stable | Opaque file bytes. |
| `Blob` | `media_type` | experimental | Display hint only. |
| `TreeEntry` | `name`, `mode`, `object_id` | stable | Canonical tree identity fields. |
| `Tree` | `entries` | stable | Entry ordering and validation are object identity concerns. |
| `PatchOp` | `address`, `op_type`, `old_data`, `new_data` | experimental | Codec-specific contract. |
| `PatchOp` | `context_hash` | experimental | Drift detection hint. |
| `Patch` | `target_path`, `codec_id`, `base_object`, `result_object`, `ops` | experimental | Patch codecs are not stable yet. |
| `Patch` | `codec_payload` | reserved | Opaque codec extension data. |
| `Revision` | `parents`, `tree`, `author`, `created_at_ms`, `summary` | stable | Core history fields. |
| `Revision` | `change_id`, `patches`, `snapshot_base`, `capsule_id`, `policy_evidence` | experimental | Intent/policy linkage is still evolving. |
| `Snapshot` | `tree_root`, `revision_id` | experimental | Snapshot objects are mostly compatibility scaffolding in v0.1. |
| `Intent` | `id`, `title`, `goal`, `status`, `created_at_ms`, `updated_at_ms` | stable | Human and agent planning contract. |
| `Intent` | `constraints`, `acceptance_tests`, `links`, `policy_refs`, `agents`, `change_ids`, `depends_on`, `supersedes` | experimental | Semantics may tighten as workflows mature. |
| `Change` | `id`, `intent_id`, `status`, `created_at_ms`, `updated_at_ms` | stable | Intent-scoped implementation attempt. |
| `Change` | `head_revision`, `workstream_id` | experimental | Branch/workstream semantics are still being shaped. |
| `Conflict` | `base_revision`, `left_revision`, `right_revision`, `file_path`, `codec_id`, `status`, `created_at_ms` | experimental | Conflict explanations and semantic regions are evolving. |
| `Conflict` | `left_patch_ids`, `right_patch_ids`, `resolution_patch_ids` | experimental | Patch-backed conflict records are codec-dependent. |
| `CapsulePublic` | `agent_id`, `evidence` | stable | Policy-visible provenance inputs. |
| `CapsulePublic` | `agent_version`, `toolchain_digest`, `env_fingerprint` | experimental | Useful metadata, not yet enforced uniformly. |
| `Evidence` | `name`, `status`, `revision_id`, `command`, `exit_code`, `runner_identity` | stable | Fresh-evidence policy inputs. |
| `Evidence` | `duration_ms`, `artifact_refs`, `summary`, `started_at_ms`, `ended_at_ms`, `environment_digest`, `log_digest`, `artifact_digest`, `expires_at_ms`, `trust_domain`, `signature` | experimental | Additional freshness and audit metadata. |
| `Capsule` | `revision_id`, `public_fields`, `signatures` | stable | Capsule verification core. |
| `Capsule` | `encrypted_private`, `encryption`, `key_id`, `recipients` | experimental | Private capsule model is supported but still being clarified. |
| `CapsuleSignature` | `signer_id`, `signature` | stable | Signature identity and bytes. |
| `CapsuleRecipient` | `recipient_id`, `key_id`, `algorithm`, `ephemeral_public_key`, `encrypted_content_key` | experimental | Recipient-envelope custody model. |
| `Policy` | `policy_id`, `required_checks`, `required_reviewers`, `visibility` | stable | Policy identity and baseline enforcement fields. |
| `Policy` | `sensitive_paths`, `quarantine_lane`, `min_trust_score`, `authorized_recipients`, `revoked_recipients`, `evidence_policy` | experimental | Security behavior is supported but pre-v1. |
| `EvidencePolicy` | all fields | experimental | Freshness semantics may tighten. |
| `Workstream` | `workstream_id`, `change_stack` | experimental | Stacked-change UX is not stable. |
| `RefLog` | `ref_name`, `entries` | internal | Storage/audit support, not a public API. |
| `RefLogEntry` | `old_target`, `new_target`, `author`, `message`, `timestamp` | internal | Storage/audit support, not a public API. |
| Merge state conflict entry | `file_path`, `conflict_id`, `codec_id`, `reason`, `regions` | experimental | Working-tree merge state, not a stored object. The explanation fields exist for CLI/operator UX and may change with codec behavior. |

## Protocol Surfaces

| Surface | Field or method group | Tier | Notes |
|---|---|---|---|
| COF header | magic `CLW1`, version byte, type tag, flags, compression marker, uncompressed length, CRC32 | stable | See [Object Format](../spec/object-format.md). |
| COF flags | `0x01` compressed, `0x02` encrypted | stable | Unknown bits are rejected. |
| COF type tags | `0x01` through `0x0c` | stable | Existing tags must not be reused. |
| CLI JSON error envelope | `schema_version`, `code`, `message`, `reason`, `request_id`, `remediation`, `exit_code`, `details` | stable | See [CLI JSON Schemas](cli-json-schemas.md). |
| CLI JSON success envelopes | `schema_version`, `action`, and command-specific top-level fields listed as stable | experimental, versioned | See [CLI JSON Schemas](cli-json-schemas.md). Nested object fields inherit their object-field tiers unless the command row says otherwise. |
| Operational evidence reports | `schema_version` or `schemaVersion`, action/status fields, command/check receipts, and pass/fail report metadata documented for release-channel and repository-health helpers | experimental, versioned | See [Release Channel Report](release-channel-report.md) and [Repository Health Report](repo-health-report.md). |
| Daemon HTTP health | `/v1/health/live`, `/v1/health/ready`, `/v1/health/deps` | beta | OpenAPI artifact is the schema source. |
| Daemon metrics | `/v1/metrics` | beta | Prometheus text shape may add metrics. |
| gRPC sync | `Hello`, `AdvertiseRefs`, `FetchObjects`, `PushObjects`, `UpdateRefs` | experimental | Compatibility checks fail closed by default. |
| gRPC intent/change/capsule/workstream/event services | all methods | experimental | Agent integration surface, not stable. |
| Plugin protocol v1 | JSON-RPC envelope, initialize handshake, error object | experimental | See plugin protocol reference. |

## Rules for New Fields

- New object fields start as experimental unless the release notes explicitly
  promote them.
- Removing or repurposing stable fields requires a compatibility note and
  migration guidance.
- Reserved fields may become stable or experimental later, but must not be used
  for an unrelated meaning.
- Internal fields can change without migration guarantees.
