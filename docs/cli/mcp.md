# `claw mcp`

Run the Claw Model Context Protocol server for local agent hosts.

```bash
claw mcp serve
claw mcp serve --allow-write
claw mcp serve --claw-binary /path/to/claw
```

`mcp serve` speaks newline-delimited JSON-RPC on stdio. By default it exposes
read-only tools for status, intents, the intent graph, deep repository health,
repair planning, story export, timeline allowance debugging, patch workbench
receipts, evidence queries, capsule inspection, review reports, agent audits,
policy simulation, and trust receipts.

Pass `--allow-write` to expose tools that create objects or run side-effectful
commands, including intent/change creation, executable acceptance tests,
provenance replay, SLSA/in-toto attestation attachment, and agent bulk lifecycle
plans. Do this only inside a trusted agent runtime.

See [MCP server reference](../reference/mcp-server.md) for method and tool
details.
