# `claw ship`

Finalize an intent/change path and produce capsule evidence for policy evaluation.

## Examples

```bash
claw ship --intent <intent-id> --evidence test=pass --evidence lint=pass
claw ship --json --intent <intent-id> --evidence test=pass --evidence lint=pass
claw ship --intent <intent-id> --evidence test=pass:1200 --co-sign <key>
claw ship --intent <intent-id> --run-acceptance
claw ship \
  --intent <intent-id> \
  --revision-ref heads/main \
  --evidence test=pass \
  --evidence-command "cargo test --workspace" \
  --runner github-actions/release \
  --environment-digest sha256:<toolchain-digest> \
  --log-digest sha256:<log-digest> \
  --evidence-expires-in-ms 86400000
claw ship \
  --intent <intent-id> \
  --evidence test=pass \
  --private-file private-capsule.json \
  --recipient-key security-reviewer:security-key:<hex-x25519-public-key>
```

Evidence should reference checks that can be rerun or audited. Policy failures return a non-zero exit and should be treated as integration blockers.

Use `--run-acceptance` to execute runnable acceptance tests linked to the intent
before creating the capsule. Each result is attached as `acceptance/N` capsule
evidence with the command, exit code, duration, revision ID, timestamps, and
`trust_domain=acceptance`. Failed or timed-out acceptance commands block
shipping before a capsule is written. Use `--acceptance-timeout-ms` to bound
each command and `--acceptance-keep-going` to collect all failures before the
ship fails.

Freshness policy fields are optional unless the referenced policy enables
`require_fresh_evidence`. When enabled, provide a trusted runner, command, exit
status implied by the evidence result, environment digest, log or artifact
digest, and an expiration window.

Private capsule metadata is encrypted when `--private-file` is used. Each
`--recipient-key` value wraps the capsule content key for one recipient ID and
must use the `recipient-id:key-id:hex-x25519-public-key` form.

## JSON Output

`claw ship --json` emits a stable v1 receipt with the revision and capsule IDs
that automation needs for release evidence:

```json
{
  "schema_version": 1,
  "action": "ship",
  "intent_id": "01H...",
  "intent_status": "Done",
  "change_id": "01H...",
  "revision_ref": "heads/main",
  "revision_id": "clw_...",
  "capsule_id": "clw_...",
  "agent_id": "claw",
  "co_signers": [],
  "signature_count": 1,
  "evidence_count": 2,
  "acceptance": {
    "run": false,
    "passed": null,
    "count": 0,
    "timeout_ms": 300000,
    "keep_going": false,
    "evidence_names": []
  },
  "private_fields_encrypted": false,
  "recipient_count": 0
}
```

Use global JSON errors for automation failures:

```bash
claw --error-format json ship --json --intent <intent-id> --evidence test=pass
```

The created revision and capsule can be inspected afterward with:

```bash
claw log --json
claw show --json <capsule-id>
```

## Exit Codes

- `0`: revision and capsule were written.
- `1`: malformed evidence/recipient input or other post-parse validation failure.
- `2`: invalid CLI usage.
- `3`: not in a Claw repository.
- `5`: object/ref/key store read or write failure.
- `10`: policy evaluation denied shipping.

## Common Errors

- Unknown intent: run `claw intent list`.
- Missing, quarantined, or revoked agent key: run `claw agent register --name <agent>`, unquarantine after investigation, or rotate the key.
- Evidence rejected by policy: include required checks, runner identity, command, digest, and expiration fields.
- Acceptance failed: run `claw intent run-acceptance <intent-id>` locally, fix the failing command, then retry `claw ship --run-acceptance`.
- Recipient key format rejected: use `recipient-id:key-id:hex-x25519-public-key`.
- Default revision ref mismatch: pass `--revision-ref <ref>` when shipping a branch other than `heads/main`.
