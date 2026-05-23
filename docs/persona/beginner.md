# Beginner

Use this path for the first five minutes with Claw.

## Five-minute demo

```bash
cargo run -p claw-vcs -- init demo-claw
cd demo-claw
claw intent create --title "Try Claw" --goal "Record intent, code, evidence, and policy"
claw change create --intent <intent-id>
printf 'hello\n' > hello.txt
claw snapshot -c <change-id> -m "Add hello file"
claw ship --intent <intent-id> --revision-ref heads/main --evidence test=pass
claw show --json heads/main
```

## Read next

- [Quickstart](../getting-started/quickstart.md)
- [Intent, change, revision](../concepts/intent-change-revision.md)
- [Capsules and evidence](../concepts/capsules-and-evidence.md)
- [Daily change workflow](../workflows/daily-change.md)
