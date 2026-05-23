# `claw show`

Inspect an object by ref, hex object ID, or `clw_` display ID.

## Examples

```bash
claw show heads/main
claw show --json heads/main
claw show <capsule-id> --decrypt-private --recipient security-reviewer --recipient-secret-key ./security-reviewer.x25519
```

`claw show` resolves refs first, then raw hex object IDs, then `clw_` display
IDs. Use it to inspect revisions, trees, blobs, patches, intents, changes,
capsules, policies, snapshots, conflicts, workstreams, and reflogs.

## JSON Output

JSON output emits `schema_version: 1` and includes the original query, object
display ID, hex ID, object type, and serialized object value. The nested
`object` block is retained for compatibility with older automation.

```json
{
  "schema_version": 1,
  "action": "show",
  "query": "heads/main",
  "object_id": "clw_...",
  "object_hex": "7b...",
  "type": "revision",
  "value": {
    "Revision": {
      "author": "claw",
      "change_id": null,
      "created_at_ms": 1710000000000,
      "parents": [],
      "patches": [],
      "policy_evidence": [],
      "summary": "initial snapshot",
      "tree": [123, 45]
    }
  },
  "object": {
    "id": "clw_...",
    "hex": "7b...",
    "type": "revision",
    "value": {
      "Revision": {
        "author": "claw",
        "change_id": null,
        "created_at_ms": 1710000000000,
        "parents": [],
        "patches": [],
        "policy_evidence": [],
        "summary": "initial snapshot",
        "tree": [123, 45]
      }
    }
  }
}
```

`--decrypt-private` is intentionally human-output only so decrypted private
fields are not accidentally piped into machine logs.

For capsule objects, human output includes recipient envelope IDs. The
`--decrypt-private` path decrypts encrypted private capsule fields with the
matching X25519 recipient secret key.

For machine-readable failures, use:

```bash
claw --error-format json show <object>
```

## Exit Codes

- `0`: object was resolved and rendered.
- `1`: object resolution or decryption failed without a more specific classification.
- `2`: invalid CLI usage, such as missing decryption arguments.
- `3`: not in a Claw repository.
- `5`: object store read failure.
- `6`: private-field decryption key material is missing from an authenticated profile path.

## Common Errors

- `cannot resolve`: verify the ref, hex object ID, or `clw_` display ID.
- `object not found`: the object ID is syntactically valid but absent from the store.
- `--decrypt-private is only supported with human-readable output`: rerun without `--json`.
- Recipient decryption failed: confirm the recipient ID, secret key file, and capsule recipient envelope.
