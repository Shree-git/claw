# `claw doctor`

Inspect CLI, repository, config, daemon, and object-store health.

```bash
claw doctor
claw doctor --json
claw doctor --strict
claw doctor --deep
```

Use `--strict` in automation when errors should fail the command. Warnings are
reported in the summary but do not make `--strict` fail. JSON output uses
schema version `1` with `action: "doctor"` and includes check names, status,
messages, remediation hints, and a summary.

Use `--deep` to scan refs, object dependencies, revision/capsule links, full
and short capsule indexes, and intent policy refs. Deep findings include safe
repair hints where available; run `claw repair plan` to inspect them before
applying changes. The JSON `deep.summary` block groups findings into the health
classes operators usually need to gate on: corruption, dangling refs, missing
capsules, capsule index drift, policy drift, weak agent keys, invalid ref
namespace entries, and repairable issue counts.

The normal `refs` check scans `.claw/refs` before object validation. It reports
manual ref files with non-portable names, unsupported entries, and sibling names
that would collide on case-insensitive filesystems, then checks that listed refs
point to local objects. The same invalid namespace entries appear in
`claw repair plan --json` as non-repairable `invalid_ref_namespace` issues so
automation can gate on them without parsing the human doctor output.

When remotes are configured, `doctor` performs a short daemon reachability
probe with the sync `Hello` request. Offline or stale remotes are reported as
warnings so local repository checks remain usable without a running daemon.
