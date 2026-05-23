# `claw sync`

Pull from or push to configured remotes.

## Examples

```bash
claw sync pull --remote origin --ref-name heads/main
claw sync pull --remote origin --ref-name heads/main --json
claw sync push --remote origin --ref-name heads/main
claw sync push --remote origin --ref-name heads/main --dry-run
claw sync push --remote origin --ref-name heads/main --dry-run --json
claw sync push --remote origin --ref-name heads/main --policy release-gate
claw sync clone <remote> <path>
claw sync clone <remote> <path> --json
claw sync pull --remote origin --intent 01H00000000000000000000000 --path-prefix src/
claw sync clone http://127.0.0.1:50051 ./partial --codec json/tree --depth 3 --byte-budget 1048576
```

`sync push --dry-run` connects to the remote, resolves the local and remote refs, and reports the object upload/ref update that would occur without mutating the remote.

Use `--policy <id>` to make push policy-aware. Claw evaluates the local revision
and its default capsule against each named repository policy before uploading
objects or updating the remote ref. Repeat the flag to require multiple
policies. A denial stops the push; with `--json`, the receipt includes policy
results and marks upload/ref update as skipped. Hosted `clawlab` HTTP remotes
that advertise capabilities must include `protocol:claw-sync/1` and
`hosted-http`. They must also advertise `policy-aware-push` before Claw will
send a policy-gated ref update; otherwise the push fails closed before the
remote ref is changed.

Production remotes should require authentication and TLS. Use the sync-level TLS flags before the subcommand:

```bash
claw sync \
  --tls-ca-cert ./ca.pem \
  --tls-domain claw.example.com \
  --client-cert ./client.pem \
  --client-key ./client-key.pem \
  push --remote https://claw.example.com:50051
```

`--client-cert` and `--client-key` must be provided together. Protocol negotiation failures should be treated as compatibility issues, not retried blindly.

## Partial Clone Filters

`sync pull` and `sync clone` can pass daemon-side fetch filters. Filtered fetches
still include dependencies needed to keep the fetched object graph usable.
If a filter excludes an advertised ref target, `sync clone` skips that ref and
leaves the working tree unmaterialized until a later unfiltered or wider fetch.
`sync pull` similarly skips the local ref update when the filtered fetch did not
include the remote target revision.

```bash
claw sync pull \
  --remote origin \
  --intent 01H00000000000000000000000 \
  --path-prefix src/ \
  --codec text/line \
  --visibility public \
  --time-start-ms 1700000000000 \
  --time-end-ms 1800000000000 \
  --depth 4 \
  --bytes 10485760
```

Available fetch filters:

- `--intent <id>`: include objects associated with an intent. Repeatable.
- `--path-prefix <path>`: include patch objects whose target path starts with a prefix. Repeatable.
- `--codec <id>`: include patch objects using a codec, such as `text/line` or `json/tree`. Repeatable.
- `--visibility <public|private|restricted>`: filter capsules by private-field visibility.
- `--time-start-ms <ms>` and `--time-end-ms <ms>`: filter revisions by creation time.
- `--depth <n>`: limit traversal depth from requested refs.
- `--bytes <n>` or `--byte-budget <n>`: stop streaming after the daemon's approximate byte budget.

JSON receipts include `filter.dimensions`, a compact list of active filter
dimensions such as `intent`, `path`, `time`, `codec`, `visibility`, `depth`,
and `byte_budget`, plus both `max_bytes` and `byte_budget` for the byte limit.

These filters are supported by gRPC remotes and by hosted `clawlab` HTTP
remotes that advertise the `hosted-http` and `partial-clone` capabilities.
Hosted remotes that do not advertise the required capability fail before
download with a clear transport error.

## JSON Output

`claw sync push --json` emits a stable v1 receipt for real pushes and dry-runs.
Dry-run receipts are useful for CI because they include the object count and ref
update that would occur without uploading objects or mutating the remote:

```json
{
  "schema_version": 1,
  "action": "sync.push",
  "dry_run": true,
  "remote": "origin",
  "ref_name": "heads/main",
  "force": false,
  "local_revision": "01H...",
  "object_count": 7,
  "policies": [],
  "remote_protocol": {
    "version": "claw-sync/1",
    "capabilities": ["protocol:claw-sync/1", "partial-clone", "event-bus", "request-limits"]
  },
  "upload": {
    "skipped": true,
    "object_count": 7,
    "message": null
  },
  "ref_update": {
    "skipped": true,
    "kind": "update",
    "old": "01H...",
    "new": "01H...",
    "success": null,
    "message": null
  }
}
```

`claw sync pull --json` emits a stable v1 receipt with the remote ref lookup,
fetch count, filter state, ref update result, and worktree update status:

```json
{
  "schema_version": 1,
  "action": "sync.pull",
  "remote": "origin",
  "ref_name": "heads/main",
  "force": false,
  "remote_ref_found": true,
  "remote_revision": "01H...",
  "fetched_count": 3,
  "target_available": true,
  "filter": {
    "active": false,
    "dimensions": [],
    "intent_ids": [],
    "path_prefixes": [],
    "codec_ids": [],
    "time_start_ms": null,
    "time_end_ms": null,
    "capsule_visibility": null,
    "max_depth": null,
    "max_bytes": null,
    "byte_budget": null
  },
  "ref_update": {
    "skipped": false,
    "old": "01H...",
    "new": "01H...",
    "success": true,
    "message": "updated"
  },
  "worktree": {
    "updated": true
  }
}
```

`claw sync clone --json` emits a stable v1 receipt with fetch totals, installed
and skipped refs, filter state, checkout status, and the local remote config
written for `origin`:

```json
{
  "schema_version": 1,
  "action": "sync.clone",
  "remote": "http://127.0.0.1:50051",
  "kind": "grpc",
  "repo": null,
  "path": "./partial",
  "fetched_count": 0,
  "remote_ref_count": 1,
  "installed_ref_count": 0,
  "skipped_refs": [
    {
      "name": "heads/main",
      "target": "01H..."
    }
  ],
  "filter": {
    "active": true,
    "dimensions": ["path"],
    "intent_ids": [],
    "path_prefixes": ["does-not-match/"],
    "codec_ids": [],
    "time_start_ms": null,
    "time_end_ms": null,
    "capsule_visibility": null,
    "max_depth": null,
    "max_bytes": null,
    "byte_budget": null
  },
  "head": "heads/main",
  "checkout": {
    "target": "01H...",
    "updated": false,
    "skipped": true
  },
  "remote_config": {
    "name": "origin",
    "path": "./partial/.claw/remotes.toml"
  }
}
```

Use global JSON errors for automation failures:

```bash
claw --error-format json sync push --remote origin --ref-name heads/main --dry-run --json
```

## Exit Codes

- `0`: sync operation or dry-run completed.
- `2`: invalid CLI usage.
- `3`: not in a Claw repository for local push/pull operations.
- `6`: missing or invalid authentication material.
- `7`: remote configuration or transport failure.
- `11`: client/server compatibility check failed.

## Common Errors

- Remote not found: run `claw remote list` or add one with `claw remote add`.
- TLS trust failure: pass `--tls-ca-cert` and `--tls-domain` for private CAs.
- mTLS pair incomplete: pass both `--client-cert` and `--client-key`.
- Auth failure: configure the token profile with `claw auth token set --stdin`.
- Protocol mismatch: upgrade/downgrade client or daemon to a compatible pair.
- Stale or missing ref: verify the remote ref name and retry after fetching current refs.
