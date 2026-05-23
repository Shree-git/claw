# Claw Playground

Open `docs/playground/index.html` in a browser to try Claw concepts without
installing the CLI.

The playground is intentionally static. It stores the current session in browser
local storage and simulates:

- intent creation
- agent-authored changes
- visual graph nodes for goals, revisions, capsules, evidence, policies, agents, and blockers
- evidence and trust receipts
- policy dry-runs
- line-based conflict detection
- portable JSON export

The playground does not write a real `.claw/` repository. Use the CLI when you
need durable objects, signatures, or policy enforcement.
