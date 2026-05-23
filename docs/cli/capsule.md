# `claw capsule`

Inspect provenance capsules without parsing raw object JSON.

## Examples

```bash
claw capsule list
claw capsule inspect heads/main
claw capsule inspect clw_...
claw capsule --json inspect clw_...
```

`inspect` accepts a capsule ref/object ID or a revision ref/object ID with an
attached capsule. Human output summarizes the revision, agent identity,
toolchain digest, environment fingerprint, evidence, signatures, private-field
state, recipients, and trust path.

JSON output uses schema version `1` with `action: "capsule.list"` or
`action: "capsule.inspect"`. It includes stable top-level capsule identity and
count fields, plus an experimental explorer section:

- `agent_identity`: claimed agent ID/version, registered-agent record, signer
  IDs, signature counts, lifecycle status, and whether the registered identity
  verified the capsule claim.
- `execution_environment`: capsule toolchain/environment fields plus unique
  runner identities, commands, environment digests, log digests, artifact
  digests, and evidence metadata coverage counts.
- `signatures[]`: signer key, signature length/prefix, cryptographic
  verification status, and matching registered agent when one exists.
- `private_fields`: encrypted/private payload state, redaction marker, envelope
  metadata, and recipient list without exposing private plaintext.
- `trust_path`: claimed agent registration, lifecycle status, verified signer
  linkage, evidence summary, private-field custody, and reasons explaining the
  path.

Treat trust-path, signature detail, and recipient-envelope metadata as
experimental in the `v0.1.x` line.
