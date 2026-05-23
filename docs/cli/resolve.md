# `claw resolve`

Manage merge conflicts created by integration workflows.

```bash
claw resolve list
claw resolve --json list
claw resolve mark <path>
claw resolve abort
```

Conflict commands are intended for review tools and agents that need explicit
conflict state rather than parsing merge text. `list` prints the conflict path,
codec, readiness state, saved explanation, and semantic regions that collided.
`mark` takes the conflicted file path and refuses to mark the file resolved
while conflict markers or sidecars are still present.

Use `--json` for agent and review-tool automation:

```bash
claw resolve --json list
claw resolve --json mark story.txt
claw resolve --json abort
```

The JSON surface is schema version `1`. `resolve.list` reports whether a merge
is in progress, the left/right/base revision metadata, conflict counts, and
conflict rows with `path`, `codec`, `status`, `has_markers`, optional `reason`,
and collided `regions`. `resolve.mark` reports the marked path and remaining
conflict count. `resolve.abort` reports the restored left ref and revision.
