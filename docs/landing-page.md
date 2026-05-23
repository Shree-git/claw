# Claw VCS

Intent. Evidence. Provenance.

Claw VCS is experimental version control for human and AI code. It keeps intent, changes, revisions, evidence, capsules, and policy in the repository so agent-authored work can be reviewed and verified offline.

## First-viewport message

**Intent-native, agent-native version control.**

Claw is for teams evaluating how source history should work when humans and AI
agents both produce code. It records why work exists, which implementation
attempt produced it, which revision was captured, and which signed evidence was
claimed for that revision.

## Why It Exists

Git remains the right default for most human-authored source history. Claw VCS explores the next layer: source-history provenance for autonomous and semi-autonomous agents.

## First Demo

```bash
claw init
claw intent create --title "Add dark mode" --goal "Support theme toggling"
claw change create --intent <intent-id>
claw snapshot --change <change-id> -m "initial implementation"
claw ship --intent <intent-id> --revision-ref heads/main --evidence test=pass --evidence lint=pass
```

Branch integration is explicit:

```bash
claw checkout main
claw integrate --right heads/dark-mode
```

## Trust Model

- Capsules say which key signed which evidence for which revision.
- Policies decide which evidence is required before integration.
- Signatures make claims attributable; they do not prove the claim is true.

## What To Show In Demo Media

- Key concepts such as intent, change, revision, capsule, policy, and refs
  should be visible in command output or narration.
- A small agent registration flow with `claw agent register --name`.
- A capsule created by `claw ship`.
- A policy object created with `claw policy create`, with a note that policies
  apply only when intents reference them.
- Git bridge output shown as experimental, not as a replacement promise.

## Primary Routes

- Static landing entrypoint: `docs/index.html`
- Browser playground: `docs/playground/index.html`
- Beginner: `docs/persona/beginner.md`
- Platform operator: `docs/persona/platform-operator.md`
- Agent developer: `docs/persona/agent-developer.md`
- Security reviewer: `docs/persona/security-reviewer.md`
- VCS nerd cave: `docs/persona/vcs-nerd-cave.md`
- New contributor: `docs/persona/contributor.md`
- Agent integrator: `docs/persona/agent-integrator.md`
- Demo script: `examples/basic-demo/scripts/demo.sh`
- Demo media: `examples/demo-media/`
- Social preview: `docs/assets/social-preview.png` (source SVG: `docs/assets/social-preview.svg`)
- Release verification: `docs/security/verifying-releases.md`
- Roadmap: `ROADMAP.md`

## Launch Status

v0.1 is experimental. Use it for local exploration, demos, and design feedback. Keep Git or another proven system as the source of truth while evaluating Claw VCS.
