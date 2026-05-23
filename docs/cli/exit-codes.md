# Claw CLI Exit Codes

Claw keeps CLI exit codes deliberate and documented for scripts, but they are
not a stable pre-1.0 compatibility contract. Clap argument parsing uses code
`2`; validations that run after parsing may return a command-specific code or
the general error code `1` until a narrower diagnostic exists.

| Code | Constant | Meaning |
| ---: | --- | --- |
| 0 | `OK` | Command completed successfully. |
| 1 | `GENERAL` | Unclassified CLI failure. |
| 2 | `USAGE` | Invalid CLI usage or argument parsing failure. |
| 3 | `NOT_REPOSITORY` | Command requires a Claw repository, but none was found. |
| 4 | `CONFIG` | Repository or CLI configuration could not be loaded or validated. |
| 5 | `IO` | Filesystem or permission failure. |
| 6 | `AUTH` | Missing or invalid authentication material. |
| 7 | `REMOTE` | Remote configuration, lookup, or transport setup failure. |
| 8 | `CONFLICT` | Merge or conflict state blocks the requested operation. |
| 9 | `WORKTREE_DIRTY` | Working tree changes block a destructive operation. |
| 10 | `POLICY` | Policy evaluation denied the requested operation. |
| 11 | `COMPATIBILITY` | CLI/daemon or protocol compatibility check failed. |

For machine-readable diagnostics, run commands with:

```console
claw --error-format json <command>
```

Human runtime diagnostics include the same `req_<milliseconds>_<counter>`
request ID as a `request_id: req_...` line so operators can correlate terminal
output with support logs.

Example envelope:

```json
{
  "schema_version": 1,
  "code": "NOT_REPOSITORY",
  "message": "not in a claw repository (no .claw directory found)",
  "reason": "Claw could not find a `.claw` directory in this path or any parent path.",
  "request_id": "req_1710000000000_0",
  "remediation": "Run `claw init` in this directory, or `cd` into an existing Claw repository.",
  "exit_code": 3,
  "details": null
}
```

Policy denials use `POLICY_DENIED`. Compatibility failures use
`COMPATIBILITY_ERROR`. Ref validation failures use `INVALID_REF_NAME` when the
requested ref is not portable, and `REF_NAME_COLLISION` when it differs from an
existing ref only by case. Those ref diagnostics currently exit with code `1`.
