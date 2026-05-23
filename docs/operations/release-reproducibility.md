# Release Reproducibility

Claw VCS does not claim fully reproducible builds yet. Each release should still publish enough metadata for reviewers to understand and independently approximate the build.

## Release Metadata

Publish with every release:

- source repository and commit SHA
- release tag
- target triples
- build command
- runner image
- Rust toolchain version
- enabled features
- checksums
- signatures
- GitHub artifact attestations
- SBOM
- release workflow file and run URL

The release workflow publishes these fields in
`claw-<tag>.release-metadata.json`. The metadata asset is checksummed, signed,
covered by SLSA provenance, covered by the release SBOM attestation, and
validated by `scripts/verify-release-channel.sh`.
The verifier's machine-readable pass/fail evidence is documented in
[Release Channel Report](../reference/release-channel-report.md).

## Local Rebuild

```bash
git clone https://github.com/Shree-git/claw-vcs.git
cd claw-vcs
git checkout <release-tag>
cargo build --release -p claw-vcs --locked
```

Compare version output:

```bash
./target/release/claw --version
```

Exact byte-for-byte equality is not guaranteed until the release process explicitly controls compiler, linker, timestamps, archive metadata, and platform-specific build inputs.
