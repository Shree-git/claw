# `claw remote`

Manage configured remotes.

```bash
claw remote list
claw remote list --json
claw remote add origin http://127.0.0.1:50051 --kind grpc --dry-run
claw remote remove origin --dry-run
```

Remote entries can point at self-hosted gRPC daemons or planned hosted ClawLab-style remotes.

`--json` emits schema version `1`. `remote list --json` reports
`action: "remote.list"`, `remote_count`, `remotes`, and `config_path`.
`remote add --json` and `remote remove --json` report `action: "remote.add"`
or `action: "remote.remove"`, the resolved remote, `dry_run`, `saved`, and
`config_path`. Each remote includes a `capabilities` object. gRPC daemon
remotes report direct `partial_clone` and `policy_aware_push` support. Hosted
`clawlab` remotes report `hosted_http: true` and mark partial clone and
policy-aware push as `requires_remote_capability`, because clients must confirm
those markers through hosted `/health` discovery before filtered pulls or
policy-gated pushes.
