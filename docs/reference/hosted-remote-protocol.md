# Hosted Remote Protocol

This page defines the ClawLab-style HTTP remote contract implemented by the
client. It is a protocol contract for compatible hosted services; it is not a
claim that a public hosted service is currently live.

## Discovery

Clients first call:

```http
GET /health
```

The response must include a `serverVersion` string and a `capabilities` array.
Hosted remotes that omit capabilities are treated as legacy and do not support
policy-aware push.

Required capability markers for the current hosted contract:

- `protocol:claw-sync/1`: sync protocol marker.
- `hosted-http`: endpoint family marker for ClawLab-style HTTP remotes.
- `partial-clone`: accepts `filter` on `/objects:batch-download`.
- `policy-aware-push`: accepts `policyReceipt` on `/refs:cas-update`.

Clients reject advertised hosted capability sets that omit either
`protocol:claw-sync/1` or `hosted-http`. Servers that omit `capabilities`
entirely are treated as legacy compatibility remotes.

Object upload performance capabilities:

- `chunked-objects`: supports `/objects:batch-upload` plus chunk upload
  sessions for large objects.
- `pack-upload`: supports `application/x-clpk` pack uploads.
- `batch-complete`: supports completing many staged object uploads in one
  request.

## Filtered Pull

Filtered pulls and clones send this request shape:

```json
{
  "want": ["<object-id>"],
  "have": ["<object-id>"],
  "cursor": null,
  "limit": 2000,
  "filter": {
    "intentIds": ["01H..."],
    "pathPrefixes": ["src/"],
    "codecIds": ["rust/ast"],
    "capsuleVisibility": "public",
    "timeRangeStart": 1700000000000,
    "timeRangeEnd": 1800000000000,
    "maxDepth": 4,
    "maxBytes": 10485760
  }
}
```

Clients fail before download when a hosted remote advertises capabilities but
does not include `partial-clone`.

## Policy-Aware Push

`claw sync push --policy <id>` evaluates policy locally before object upload and
ref update. For hosted HTTP remotes, the ref update request also carries the
local policy receipt:

```json
{
  "updates": [
    {
      "name": "heads/main",
      "oldTarget": "<old-object-id>",
      "newTarget": "<new-object-id>",
      "force": false
    }
  ],
  "policyReceipt": {
    "policies": [
      {
        "id": "release-gate",
        "ref": "refs/policies/release-gate",
        "object": "<policy-object-id>",
        "allowed": true
      }
    ],
    "requestedCapabilities": ["protocol:claw-sync/1", "partial-clone", "policy-aware-push"],
    "negotiatedCapabilities": ["protocol:claw-sync/1", "partial-clone", "policy-aware-push"]
  }
}
```

Hosted clients fail closed before changing refs when policy checks are present
and the remote does not advertise `policy-aware-push`.

## Replay And Auth

Mutating hosted requests include both `idempotency-key` and
`x-claw-replay-nonce`. Bearer tokens are sent with `Authorization: Bearer`.
Servers should scope replay detection to principal, token ID, action, and
resource/ref target, matching the gRPC daemon behavior.
