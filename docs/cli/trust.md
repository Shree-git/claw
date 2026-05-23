# `claw trust`

Explain why a revision is or is not trustworthy under stored policies.

## Examples

```bash
claw trust receipt --revision heads/main
claw trust receipt --revision heads/main --policy default
claw trust --json receipt --revision heads/main --signer-agent ci-agent
claw trust receipt --revision heads/main --path src/lib.rs --trust-score 90%
```

`receipt` loads the revision, resolves its capsule unless `--capsule` is
provided, evaluates the selected policies, and prints the evidence, signature,
trust-score, capsule trust path, and policy verdicts that explain the result.

If no `--policy` is provided, all stored policies are evaluated. Use
`--signer-agent`, `--signer-key`, `--path`, and `--trust-score` to provide the
policy context that is normally supplied by an operator or integration system.

JSON output uses schema version `1` with `action: "trust.receipt"` and includes
`trustworthy`, `summary`, `why`, `revision`, `capsule`, `provenance`,
`context`, and `policies`. `trustworthy` requires both passing policy
evaluation and a capsule trust path with cryptographic integrity plus a verified
registered agent identity. The `summary` block gives policy, signature,
evidence, trust-score, and private-recipient counts; `why` is the concise
human-readable explanation. The `provenance` block summarizes revision-level
policy evidence, capsule evidence, replay evidence, and SLSA/in-toto
attestation evidence. The `capsule` field uses the same explorer payload as
`claw capsule --json inspect`, including registered agent identity, toolchain
and environment metadata, evidence, signature verification, private-field
custody, and trust-path reasons.
