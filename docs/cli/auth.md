# `claw auth`

Manage saved authentication profiles for hosted or daemon-backed remotes.

```bash
claw auth login --base-url https://auth.example.invalid --profile prod
printf '%s\n' "$CLAW_TOKEN" | claw auth token set --stdin --base-url https://daemon.example.invalid --profile prod
printf '%s\n' "$CLAW_TOKEN" | claw auth --json token set --stdin --base-url https://daemon.example.invalid --profile prod
claw auth token show --profile prod
claw auth token list
claw auth logout --profile prod
```

Hosted-service auth is not configured by default in v0.1. Pass `--base-url`
for a self-hosted endpoint or for a hosted service that release notes explicitly
mark as live.

Tokens are local credential material. Do not commit `~/.claw/auth.toml`, `~/.claw/auth.key`, copied bearer tokens, or support bundles containing auth data.

## JSON Output

Use `auth token set --stdin` in automation so bearer tokens do not appear in
shell history or process arguments. The positional token form remains available
for local ad hoc testing.

`--json` emits schema version `1` receipts for `login`, `logout`, `token set`,
`token show`, and `token list`. Token values and token prefixes are never
printed; receipts expose only profile names, base URLs, token source,
token-presence booleans, and expiry metadata.

Use global JSON errors for automation failures:

```bash
claw --error-format json auth --json token show --profile prod
```
