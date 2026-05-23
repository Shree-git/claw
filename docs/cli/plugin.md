# `claw plugin`

Manage external policy or codec plugin integrations.

```bash
claw plugin check --plugin ./target/debug/claw-policy-plugin
claw plugin --json check --plugin ./target/debug/claw-policy-plugin
```

Plugins communicate through structured protocol messages. Treat plugin stderr as diagnostics and keep policy decisions in machine-readable output.

Use `--json` when plugin compliance checks are part of CI or release evidence.
The `plugin.check` receipt is schema version `1` and includes `ok`, `plugin`,
`protocol`, `timeout_ms`, `request_id`, `jsonrpc`, and `method`. It does not
record plugin stdout or stderr streams.
