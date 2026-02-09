# Contributing to Claw

Thanks for your interest in contributing.

## Prerequisites

- Rust toolchain via `rustup` (stable channel)
- Git
- Optional: `protoc` (Protocol Buffers compiler) if you want to override the vendored one (set `PROTOC=/path/to/protoc`)

## Local Setup

```bash
git clone https://github.com/shree-git/claw.git
cd claw
cargo build
```

## Development Workflow

1. Fork the repository.
2. Create a branch from `main`.
3. Make focused changes.
4. Run required checks locally.
5. Open a pull request against `main`.

## Required Checks Before PR

Run all of the following and ensure they pass:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Pull Request Expectations

- Keep each PR scoped to a clear goal.
- Include tests for behavior changes.
- Update docs when commands, behavior, or project policies change.
- Explain motivation and design tradeoffs in the PR description.

## Legal / Attestation

- No CLA is required.
- No DCO sign-off is required.

By contributing, you agree your contributions are licensed under the repository
license (`MIT`).
