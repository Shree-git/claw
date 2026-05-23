# `claw provenance`

Replay claimed evidence and attach supply-chain attestations to capsules.

## Examples

```bash
claw provenance replay --revision heads/main --evidence test --dry-run
claw provenance --json replay --revision heads/main --keep-going
claw provenance replay --revision heads/main --in-place --dry-run
claw provenance attach-attestation --revision heads/main --file provenance.intoto.json
claw provenance --json attach-attestation \
  --revision heads/main \
  --file slsa.json \
  --subject-name artifact.tar.gz \
  --subject-digest sha256:... \
  --builder-id https://github.com/actions/runner \
  --build-type https://github.com/Actions
```

`replay` re-runs command-backed evidence from the selected capsule, compares the
actual exit status with the capsule claim, and also compares the actual
stdout/stderr digest when the original evidence claimed a `log_digest`. It then
adds `replay:<name>` evidence to a newly signed replacement capsule. By default, replay uses an isolated
filesystem sandbox: Claw copies the current worktree to a temporary directory
without `.claw` or `.git` metadata and runs evidence commands there. Use
`--in-place` only when the command must inspect local repository metadata. Use
`--dry-run` to inspect comparison output without updating capsule refs.

`attach-attestation` reads an in-toto statement or SLSA provenance JSON file,
validates optional subject constraints, validates SLSA predicate metadata,
records the attestation digest, and adds the claim as capsule evidence. SLSA
provenance must include `predicate.builder.id` and `predicate.buildType`; use
`--builder-id` and `--build-type` to require exact expected values. When
`--subject-digest` uses `algorithm:value` syntax, the subject must contain a
matching digest under that algorithm key; raw digest values match any subject
digest algorithm. The updated capsule is signed by the selected agent and
replaces the default capsule refs for the revision.

With `--json`, provenance commands emit `schema_version: 1` and an `action`
field. `replay` uses `action: "provenance.replay"` and returns `replay` and
`attachment` objects; replay receipts include the workspace mode, per-row
`workspace` values, selected/replayed/matched/mismatched counts, replay
start/end timestamps, the actual replay log digest, and digest match status when
the capsule claimed a log digest. Replay evidence written back to the capsule
also carries the replay start/end timestamps. `attach-attestation` uses
`action: "provenance.attach_attestation"` and returns `attestation` and
`attachment` objects. The `attestation` object includes the statement type,
predicate type, subject count, builder id, build type, and validation details.
`--dry-run` still validates input and reports the evidence that would be added,
but it does not update capsule refs or create a signing agent.
