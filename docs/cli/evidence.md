# `claw evidence`

Query evidence claims carried by provenance capsules.

## Examples

```bash
claw evidence query 'test=pass'
claw evidence query 'test=pass AND runner=github-actions/release'
claw evidence query '(test=pass OR lint=pass) AND trust>=0.8'
claw evidence --json query 'trust>=0.8 AND status=pass' --limit 10
claw evidence query 'lint=pass' --revision heads/main
claw evidence query 'test=pass' --capsule clw_...
```

Queries are boolean expressions over evidence, capsule, signer, and revision
fields. Use `AND`, `OR`, and parentheses to group clauses. Supported comparison
operators are `=`, `==`, `!=`, `>`, `>=`, `<`, and `<=`; quoted values may
contain spaces.

Useful fields include `name`, `status`, `summary`, `duration_ms`, `runner`,
`command`, `exit_code`, `started_at_ms`, `ended_at_ms`, `expires_at_ms`,
`environment_digest`, `log_digest`, `artifact_digest`, `trust_domain`,
`signature`, `agent`, `signer`, `trust`, `revision`, `evidence.revision_id`,
and `artifact_ref`. A bare evidence name such as `test=pass` is a shorthand for
matching an evidence item by name and status.

JSON output uses schema version `1` with `action: "evidence.query"` and
includes `query`, `query_plan`, `count`, and `matches`. The query plan lists
normalized fields, operators, boolean operators, filters, and whether the query
uses signer trust, capsule, revision, or evidence fields. Each match also
includes `matched_clauses` with the matching field, operator, expected value,
and actual values from the evidence row or capsule, so expressions such as
`test=pass AND signer.trust>0.8` can be audited without reparsing the query.
