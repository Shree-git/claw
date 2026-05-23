# Plugin Protocol v1

This document defines the **process-isolated JSON-RPC v1** protocol used by `claw` plugins.

## 1) Transport and lifecycle

- Host and plugin communicate over `stdin`/`stdout` using line-delimited UTF-8 JSON messages.
- Each plugin runs as its own OS process. The host must not load plugin code in-process.
- `stderr` is reserved for diagnostics and must not carry protocol messages.
- The host starts the plugin executable and sends a `plugin.initialize` request first.
- Either side may terminate after an unrecoverable protocol error.

## 2) Message format (JSON-RPC v1 profile)

Every message is a JSON object:

```json
{
  "jsonrpc": "1.0",
  "id": "req-123",
  "method": "plugin.initialize",
  "params": {"key": "value"}
}
```

Response:

```json
{
  "jsonrpc": "1.0",
  "id": "req-123",
  "result": {"ok": true},
  "error": null
}
```

Error response:

```json
{
  "jsonrpc": "1.0",
  "id": "req-123",
  "result": null,
  "error": {
    "code": "INVALID_PARAMS",
    "message": "missing field: target",
    "data": {"field": "target"}
  }
}
```

Rules:

- `jsonrpc` must be exactly `"1.0"`.
- `id` is required for requests and responses; notifications omit `id`.
- Exactly one of `result` or `error` must be non-null in responses.

## 3) Capability negotiation

Initialization handshake:

1. Host sends `plugin.initialize`.
2. Plugin replies with negotiated capabilities.
3. Host may call `plugin.ready` notification to begin normal operation.

`plugin.initialize` request (host -> plugin):

```json
{
  "jsonrpc": "1.0",
  "id": "init-1",
  "method": "plugin.initialize",
  "params": {
    "protocolVersion": "1",
    "host": {"name": "claw", "version": "<host-version>"},
    "capabilities": {
      "streaming": false,
      "tools": ["exec", "read"],
      "maxRequestTimeoutMs": 60000
    }
  }
}
```

`plugin.initialize` response (plugin -> host):

```json
{
  "jsonrpc": "1.0",
  "id": "init-1",
  "result": {
    "plugin": {"name": "example-plugin", "version": "0.1.0"},
    "capabilities": {
      "streaming": false,
      "tools": ["exec"],
      "maxRequestTimeoutMs": 30000
    }
  },
  "error": null
}
```

Negotiation contract:

- Effective capability set is the intersection of host-offered and plugin-supported features.
- If `protocolVersion` is unsupported, plugin must return `UNSUPPORTED_VERSION`.
- Host may downgrade optional features, but must not invoke features outside the negotiated set.

## 4) Timeout and sandbox contract

- All host requests include an implicit or explicit timeout budget.
- Plugin must stop work and return `TIMEOUT` when it cannot complete within budget.
- Host may forcibly terminate plugin process after timeout grace period.
- Plugin executes inside host-defined sandbox constraints (filesystem, network, process, env).
- Plugin must treat denied operations as normal runtime failures and return a structured error.

Recommended request parameter shape for budget-aware methods:

```json
{
  "timeoutMs": 10000,
  "sandbox": {
    "fs": "workspace-readwrite",
    "network": "restricted",
    "exec": false
  }
}
```

## 5) Error semantics

Errors are protocol-level and method-level, returned in `error`:

| Code | Meaning | Retry |
|---|---|---|
| `PARSE_ERROR` | Invalid JSON payload | No |
| `INVALID_REQUEST` | Malformed JSON-RPC envelope | No |
| `METHOD_NOT_FOUND` | Unknown method | No |
| `INVALID_PARAMS` | Missing or invalid params | No |
| `UNSUPPORTED_VERSION` | Protocol version mismatch | No |
| `TIMEOUT` | Request exceeded timeout budget | Maybe |
| `SANDBOX_DENIED` | Blocked by sandbox policy | No |
| `INTERNAL_ERROR` | Plugin internal failure | Maybe |

Semantics:

- `message` is human-readable and stable enough for logs.
- `data` is optional machine-readable context.
- For idempotent methods, `TIMEOUT` and `INTERNAL_ERROR` may be retried by host policy.

## 6) Minimal method set

- `plugin.initialize` (request): capability and version negotiation.
- `plugin.ready` (notification): host signals post-init state.
- `plugin.shutdown` (request): graceful termination.
- `plugin.health` (request, optional): liveness/readiness diagnostics.

## 7) Compliance requirements

A v1-compliant plugin must:

- Implement JSON-RPC v1 envelope rules in this document.
- Support `plugin.initialize` and `plugin.shutdown`.
- Enforce timeout handling and produce structured errors.
- Operate correctly under process isolation and sandbox restrictions.

Use `claw plugin --json check --plugin <path>` to record a machine-readable
initialize-handshake receipt in CI or release evidence. The receipt uses CLI
schema version `1` with `action: "plugin.check"`.
