# Agent Developer

Use this path when an agent, CI worker, or automation service creates Claw
objects or consumes Claw JSON.

## Start

- [Agent integration guide](../agents/integration-guide.md)
- [Agent change workflow](../agents/change-workflow.md)
- [Evidence schema](../agents/evidence-schema.md)
- [CLI JSON schemas](../reference/cli-json-schemas.md)

## Rules of thumb

- Use `--json` for successful command output and `--error-format json` for
  runtime errors.
- Preserve intent IDs, change IDs, revision IDs, capsule IDs, and policy IDs in
  agent logs.
- Stop on policy denial. Add missing evidence or signatures; do not retry with
  weaker evidence.
- Treat private capsule fields as encrypted-only data. Do not put secrets in
  public evidence fields.
