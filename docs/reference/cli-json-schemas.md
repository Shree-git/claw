# CLI JSON Schemas

This page defines the machine-readable CLI JSON surfaces that automation may
parse in the `v0.1.x` line. These schemas are versioned even while the overall
CLI remains experimental.

## Error Envelope

Emit with:

```console
claw --error-format json <command>
```

Schema version: `1`

| Field | Type | Tier | Meaning |
|---|---|---|---|
| `schema_version` | integer | stable | Error-envelope schema version. |
| `code` | string | stable | Machine-readable error code. |
| `message` | string | stable | Human-readable summary of what happened. |
| `reason` | string | stable | Human-readable explanation of why Claw classified the failure this way. |
| `request_id` | string | stable | Per-error diagnostic ID in `req_<milliseconds>_<counter>` form, unique within the CLI process. |
| `remediation` | string or null | stable | Next exact command or command pattern to try. |
| `exit_code` | integer | stable | Process exit code. |
| `details` | object, array, scalar, or null | experimental | Command- or parser-specific details. |

Example:

```json
{"schema_version":1,"code":"NOT_REPOSITORY","message":"not in a claw repository","reason":"Claw could not find a `.claw` directory in this path or any parent path.","request_id":"req_1779240000000_0","remediation":"Run `claw init` in this directory, or `cd` into an existing Claw repository.","exit_code":3,"details":null}
```

Stable error codes include `NOT_REPOSITORY`, `USAGE_ERROR`, `CONFIG_ERROR`,
`AUTH_ERROR`, `REMOTE_ERROR`, `WORKTREE_DIRTY`, `CONFLICT_STATE`,
`POLICY_DENIED`, `COMPATIBILITY_ERROR`, `IO_ERROR`, `INVALID_REF_NAME`,
`REF_NAME_COLLISION`, and `CLI_ERROR`. For `INVALID_REF_NAME`, `details.ref_name`
contains the rejected ref. For `REF_NAME_COLLISION`, `details.requested` and
`details.existing` describe the case-insensitive collision.

## Success Schemas

Command success JSON is command-specific. Top-level keys listed here are the
documented automation contract; nested object fields inherit the object-field
tiers in [Object and Protocol Stability Tiers](object-stability-tiers.md).

| Command | Schema version | Stable top-level fields | Experimental top-level fields |
|---|---:|---|---|
| `claw version --json` | `1` | `schema_version`, `action`, `name`, `version`, `package`, `object_format_version`, `sync_protocol_version`, `sync_capabilities`, `build`, `os`, `arch` | `git_sha` |
| `claw auth --json login/logout/token` | `1` | `schema_version`, `action`; login/set/show/list/logout receipts include `profile`; login/set/show/list include `base_url` or `profiles`; Token values and token prefixes are never emitted; `token_source`, `token_present`, `refresh_token_present`, `expires_at_unix`, `profile_count`, and `removed` describe credential state | hosted-login provider metadata |
| `claw doctor --json` | `1` | `schema_version`, `action`, `version`, `cwd`, `repo_root`, `checks`, `summary` | `deep` health report with category summary counts for corruption, invalid ref namespace entries, dangling refs, missing capsules, capsule index drift, policy drift, weak keys, repairability, and check-specific remediation text |
| `claw status --json` | `1` | `schema_version`, `action`, `branch`, `head`, `in_merge`, `changes` | none |
| `claw init --json` | `1` | `schema_version`, `action`, `initialized`, `path`, `claw_dir`, `dry_run`, `already_initialized`, `created`, `head`, `next_steps` | none |
| `claw snapshot --json` | `1` | `schema_version`, `action`, `snapshot_created`, `revision_id`, `branch`, `merge_resolved` | `patches`, `changed_files`, `parents`, `reason` |
| `claw log --json` | `1` | `schema_version`, `action`, `ref`, `all`, `limit`, `tip_count`, `entry_count`, `entries`; each entry has `revision_id`, `author`, `created_at_ms`, `summary`, `parents` | entry `change_id`, `intent_title`, `capsule_id` |
| `claw show --json <ref>` | `1` | `schema_version`, `action`, `query`, `object_id`, `object_hex`, `type`, `value`, `object` | none |
| `claw diff --json` | `1` | `schema_version`, `action`, `from`, `to`, `path_filter`, `change_count`, `changes` | object IDs inside each change |
| `claw patch --json codecs/create/apply/show/invert/commute/merge3/workbench` | `1` | `schema_version`, `action`, plus codec inventory/resolution fields or `patch`, `codec`, `ops`, `applied`, `file`, `bytes_written`, `merged`, `commutes`, or `workbench` as applicable; codec inventory rows include `operation_model` and `canonical_output`; workbench receipts include `analysis` with path/codec/base/result comparability, operation addresses, overlap, reorder, invertibility, and decision fields | codec-specific operation payloads and workbench explanations |
| `claw plugin --json check` | `1` | `schema_version`, `action`, `ok`, `plugin`, `protocol`, `timeout_ms`, `request_id`, `jsonrpc`, `method`; `action` is `plugin.check`; `method` is `plugin.initialize` | future negotiated capability summaries |
| `claw capsule --json list/inspect` | `1` | `schema_version`, `action`, `count`, `capsules` for `list`; `id`, `hex`, `revision_id`, `agent_id`, `evidence_count`, `signature_count`, `has_private_fields` for `inspect` | `agent_identity`, `execution_environment`, signature verification, registered-agent linkage, `private_fields`, `recipients`, and `trust_path` metadata |
| `claw evidence --json query` | `1` | `schema_version`, `action`, `query`, `query_plan`, `count`, `matches`; `query_plan` includes normalized fields, operators, boolean operators, filters, and signer-trust/capsule/revision/evidence usage flags; each match includes `matched_clauses` with expected and actual values | matcher-specific embedded evidence fields |
| `claw review --json` | `1` | `schema_version`, `action`, `intent_count`, `change_count`, `capsule_count`, `filters`, `summary`, `index`, `intents`; `summary` contains review-required, blocker, missing policy/capsule, unsigned/private capsule, and evidence counts; `index` contains flattened `changes` and `capsules` review units | embedded intent, change, revision, capsule, and policy fields |
| `claw story export --format json` | `1` | `schema_version`, `action`, `summary`, `narrative`, `intent`, `changes`; `summary` contains change/revision/capsule/signature/evidence/policy counts and `trust_posture`; `narrative` contains `audience_summary`, `audit_verdict`, `risk_notes`, `next_actions`, and `timeline` | embedded revision and capsule fields |
| `claw trust --json receipt` | `1` | `schema_version`, `action`, `trustworthy`, `summary`, `why`, `revision`, `capsule`, `provenance`, `context`, `policies` | policy-specific denial reasons, revision policy evidence, capsule evidence, trust-score inputs, and the embedded capsule explorer trust path |
| `claw timeline --json ref/revision/allowed` | `1` | `schema_version`, `action`, `ref`, `entries` for `ref`; `revision` for `revision`; `revision`, `allowed_by`, `allowed_by_policy_ids`, `denied_by_policy_ids`, `first_seen_at_ms`, `first_allowed_at_ms`, `first_allowed_by_policy_id`, `first_allowed_policy_ref`, `ref_events`, `policies` for `allowed`; per-policy rows include `policy_first_seen_at_ms`, `policy_first_seen_ref`, `allowed_since_ms`, and `allowed_since_basis` | reflog messages and embedded policy denial reasons |
| `claw repair --json plan/apply` | `1` | `schema_version`, `action`, `object_count`, `ref_count`, `issue_count`, `repairable_count`, `summary`, `issues` for `plan`; `dry_run`, `planned_count`, `applied_count`, pre-apply `summary`, nullable `post_summary`, nullable `remaining_issue_count`, nullable `remaining_repairable_count`, `planned`, `applied` for `apply` | issue-specific repair payloads, including `capsule_reindex`, `ref_rollback`, `ref_recovery`, `policy_audit_regeneration`, non-repairable `invalid_ref_namespace`, and manual `object_restore` plans |
| `claw provenance --json replay/attach-attestation` | `1` | `schema_version`, `action`, `attachment`; `replay` with workspace mode/results for `action: "provenance.replay"`; replay rows include status, exit, actual log digest, and optional log digest match fields; `attestation` for `action: "provenance.attach_attestation"` includes status, summary, statement/predicate type, subject count, builder id, build type, and validation details | replay row timings, sandbox metadata, attestation summaries, and attachment capsule IDs |
| `claw admin --json preflight/migrate/backup/rollback/support-bundle` | `1` | `schema_version`, `action`; `ok`, `repo_root`, `config_path`, `summary`, and `checks` for preflight; `dry_run`, `applied`, `backup_id`, `target`, `source`, `source_kind`, and `diff` for migration commands; `backup_id`, `file_count`, and `total_bytes` for `backup create`; `backup_id` and `backup` for `backup verify`; `backup_id`, `verified`, plus rollback result fields for rollback commands; `written`, `path`, `request_id`, `created_at_ms`, `refs_count`, `latest_backup_id`, and `redaction_count` for support bundles; support bundle files include `redactions` and redact TLS config paths | filesystem paths to manifests and snapshots, preflight check details, migration diffs |
| `claw ship --json` | `1` | `schema_version`, `action`, `intent_id`, `intent_status`, `change_id`, `revision_ref`, `revision_id`, `capsule_id`, `agent_id`, `co_signers`, `signature_count`, `evidence_count`, `private_fields_encrypted`, `recipient_count` | future policy receipt summaries and verifier hints |
| `claw integrate --json` | `1` | `schema_version`, `action`, `dry_run`, `clean`, `left_ref`, `right_ref`, `left_revision`, `right_revision`, `base_revision`, `result_revision`, `result_tree`, `ref_updated`, `worktree_updated`, `merge_state_written`, `conflict_count`, `conflicts`; each conflict has `path`, `codec`, `reason`, `regions` | future policy evaluation summaries and merge strategy diagnostics |
| `claw resolve --json list/mark/abort` | `1` | `schema_version`, `action`; `resolve.list` has `merge_in_progress`, nullable left/right/base refs and revisions, `conflict_count`, `ready_count`, `unresolved_count`, and `conflicts`; each conflict has `path`, `conflict_id`, `codec`, `status`, `has_markers`, `reason`, and `regions`; `resolve.mark` has `path`, `marked`, `remaining_conflict_count`, and `merge_complete`; `resolve.abort` has `aborted`, `restored_ref`, `restored_revision`, and `conflict_count` | future conflict editor hints |
| `claw sync push/pull/clone --json` | `1` | `schema_version`, `action`; push has `dry_run`, `remote`, `ref_name`, `force`, `local_revision`, `object_count`, `policies`, `remote_protocol`, `upload`, `ref_update`; pull has `remote`, `ref_name`, `force`, `remote_ref_found`, `remote_revision`, `fetched_count`, `target_available`, `filter`, `ref_update`, `worktree`; clone has `remote`, `kind`, `repo`, `path`, `fetched_count`, `remote_ref_count`, `installed_ref_count`, `skipped_refs`, `filter`, `head`, `checkout`, `remote_config`; `filter` has `active`, `dimensions`, intent/path/time/codec/visibility/depth fields, `max_bytes`, and `byte_budget`; `remote_protocol` has `version`, `capabilities`; `upload` has `skipped`, `object_count`, `message`; `ref_update` has `skipped`, `old`, `new`, `success`, `message`; `checkout` has `target`, `updated`, `skipped` | future remote-side receipt IDs and checkout diagnostics |
| `claw bridge --json import` | `1` | `schema_version`, `action`, `provider`, `dry_run`, `revision`, `raw_import_ref`, `intent`, `change`, `capsule`, `policy`, `evidence_added`, `notes_imported`, `mapping`; `mapping` includes PR/MR, checks, statuses, reviews, notes, branch-protection, required-check, and hosted review-rule coverage | normalized live-provider fetch payloads and provider-specific object refs |
| `claw git-export --json` | `1` | `schema_version`, `action`, `dry_run`, `git_dir`, `export_count`, `exports`; `action` is `git-export`; each export has `source_ref`, `revision_id`, `git_branch`, `revision_count`, `git_commit`, `note_count` | notes metadata and future Git object summary fields |
| `claw git-import --json` | `1` | `schema_version`, `action`, `dry_run`, `git_dir`, `import_count`, `imports`; `action` is `git-import`; each import has `git_ref`, `git_commit`, `claw_ref`, `revision_id` | notes metadata and importer diagnostic fields |
| `claw git-roundtrip --json` | `1` | `schema_version`, `action`, `verified`, `source_ref`, `source_revision`, `git_dir`, `exported_git_ref`, `exported_git_commit`, `import_ref`, `imported_revision`, `with_notes`, `notes_ref`, `notes_imported`, `checks`; `action` is `git-roundtrip`; `checks` has `tree`, `change_linkage`, `ancestry`, `source_revision_count`, and `imported_revision_count` | per-object mismatch diagnostics when verification fails |
| `claw migration wizard --json` | `1` | `schema_version`, `action`, `dry_run`, `git_dir`, `branch_count`, `branches`, `metadata_summary`, `notes_imported`, `metadata_ref`, `policy_suggestion_ref`, `suggested_policy`; `metadata_summary` includes metadata/branch-protection coverage and review-rule counts; `suggested_policy` includes required checks/reviewers, review requirements, migration warnings, sensitive paths, trust score, and optional object id; each branch has `git_ref`, `claw_ref`, `revision_id`, `intent_id`, `change_id`, `title`, `metadata_match`, `metadata_links` | inferred branch intent/change IDs, metadata inference coverage, and policy suggestions |
| `claw mcp serve` | MCP JSON-RPC | `initialize`, `tools/list`, `tools/call`, and `ping` response envelopes | tool-specific `structuredContent` mirrors the delegated CLI JSON command output |
| `claw branch --json` | `1` | `schema_version`, `action`, `current`, `branch_count`, `branches` for list; `schema_version`, `action`, `branch`, `ref`, `target`, `dry_run`, `created` for create; `schema_version`, `action`, `branch`, `ref`, `target`, `dry_run`, `deleted` for delete | `unborn` branch marker |
| `claw checkout --json <target>` | `1` | `schema_version`, `action`, `target`, `target_id`, `checked_out`, `dry_run`, `detached`, `files_written`, `updated` | none |
| `claw intent --json ...` | `1` | `schema_version`, `action`, `intent` for create/show/update; `schema_version`, `action`, `intent_count`, `intents` for list; graph includes `node_count`, `edge_count`, `nodes`, `edges`, `intents`, and `changes`; acceptance and policy receipts include `schema_version` and namespaced `action` | embedded intent fields marked experimental |
| `claw change --json ...` | `1` | `schema_version`, `action`, `change` for create/show/status; `schema_version`, `action`, `change_count`, `filters`, `changes` for list | embedded change fields marked experimental |
| `claw remote --json ...` | `1` | `schema_version`, `action`, `config_path`, `remote_count`, `remotes` for list; `schema_version`, `action`, `remote`, `dry_run`, `saved`, `config_path` for add/remove; remote entries include `capabilities` with transport, hosted HTTP, partial-clone, policy-aware-push, discovery, and marker fields | remote transport details |
| `claw policy eval/simulate ... --json` | `1` | `schema_version`, `action`, `allowed`, `error`, `policy`, `revision`, `capsule`, `context`, `simulation`; `capsule` includes nullable `id`/`hex`, source, and synthetic-missing marker; `context` includes signer IDs, trust score, touched paths, derived revision patch paths, and touched-path source; `simulation` includes pass/fail counts, first failure, failed steps, and ordered gate steps; `policy.source` is `stored` or `file` | per-step policy gate inputs and external plugin details |
| `claw policy apply ... --json` | `1` | `schema_version`, `action`, `dry_run`, `ref`, `old_object`, `new_object`, `policy` | embedded policy fields |
| `claw policy lint ... --json` | `1` | `schema_version`, `action`, `policy_count`, `finding_count`, `danger_count`, `warning_count`, `policies` | finding codes and remediation text |
| `claw agent audit --json` | `1` | `schema_version`, `action`, lifecycle counts, local-key counts, fleet triage counts, `filters`, `matching_agents`, `agents`, `findings`; each agent has `agent_id`, `ref_name`, `object_id`, `record_state`, `status`, `local_key_state`, `risk_level`, `action_required`, `recommended_action`, key metadata, timestamps, and quarantine/revocation metadata | finding text and future custody checks |
| `claw agent --json register/keygen/rotate/revoke/quarantine/unquarantine/bulk/status/list` | `1` | `schema_version`, `action`, lifecycle result fields for register/keygen/rotate/revoke/quarantine/unquarantine/status; `bulk` has `dry_run`, `planned_count`, `changed_count`, and per-operation `results`; `schema_version`, `action`, `agent_count`, `agents` for list | key lifecycle metadata |

## Compatibility Rules

- Additive fields are allowed on experimental schemas.
- Stable top-level fields cannot change type without a release-note warning and
  a migration path.
- Automation should ignore unknown fields and should pin the Claw version for
  production workflows until the CLI surface is promoted from experimental.
