# `claw daemon`

Run the gRPC sync daemon and HTTP health listener.

```bash
claw daemon --listen 127.0.0.1:50051 --health-listen 127.0.0.1:50052
claw daemon --auth-profile prod --auth-role writer --auth-scope capsules:private-read
printf '%s\n' "$CLAW_TOKEN" | claw daemon --auth-token-stdin --auth-role writer
claw daemon --rate-limit-per-minute 600 --max-push-chunk-bytes 8388608 --max-push-request-bytes 134217728
claw daemon --tls-cert server.pem --tls-key server-key.pem --client-ca-cert ca.pem
claw daemon --health-listen 0.0.0.0:50052 --allow-public-health
claw serve
```

In the production profile, non-local binds require authentication and TLS when
the default hardened config is active. Bearer-authenticated gRPC calls are
authorized by role/scope grants across sync, intent, change, capsule,
workstream, and event-stream services. `claw serve` is an alias for
`claw daemon`.

Prefer `--auth-profile` or `--auth-token-stdin` for daemon startup so bearer
tokens do not appear in shell history or process arguments. `--auth-token` is
kept for compatibility and local ad hoc testing.

The HTTP health listener also serves `/v1/metrics`. In the production profile,
binding that listener beyond localhost requires the explicit
`--allow-public-health` opt-in.
