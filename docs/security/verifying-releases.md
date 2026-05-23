# Verifying Releases

Use these checks before installing a downloaded Claw VCS release artifact.

## Expected Identity

- Repository: `Shree-git/claw-vcs`
- Binary name: `claw`
- Release tags: `vX.Y.Z`
- Artifact signing: Sigstore/Cosign when release assets provide signatures
- Provenance: GitHub artifact attestations when release assets provide attestations

## Checksums

Download the archive and checksum file from the same GitHub Release. To verify
one downloaded asset without downloading every file named in `sha256.sum`, check
only that asset's line:

```bash
grep '  claw-x86_64-unknown-linux-gnu.tar.xz$' sha256.sum | sha256sum -c -
```

On macOS:

```bash
grep '  claw-x86_64-unknown-linux-gnu.tar.xz$' sha256.sum | shasum -a 256 -c -
```

If you downloaded the full release asset set, `sha256sum -c sha256.sum` or
`shasum -a 256 -c sha256.sum` should also pass.

## Cosign Blob Signatures

When a release provides `.sig` and certificate material:

```bash
cosign verify-blob \
  --signature ./claw-x86_64-unknown-linux-gnu.tar.xz.sig \
  --certificate ./claw-x86_64-unknown-linux-gnu.tar.xz.pem \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github.com/Shree-git/(claw|claw-vcs)/\.github/workflows/release\.yml@refs/tags/vX.Y.Z$' \
  ./claw-x86_64-unknown-linux-gnu.tar.xz
```

The certificate issuer must be GitHub Actions OIDC, and the certificate
identity must point at this repository's workflow on the exact release tag.

## GitHub Artifact Attestations

When a release provides GitHub artifact attestations:

```bash
gh attestation verify ./claw-x86_64-unknown-linux-gnu.tar.xz --repo Shree-git/claw-vcs
```

Verify the attestation references the expected repository, commit SHA, workflow, and tag.
For launch releases, verify both provenance and SBOM attestations by pinning
predicate types:

```bash
gh attestation verify ./claw-x86_64-unknown-linux-gnu.tar.xz \
  --repo Shree-git/claw-vcs \
  --predicate-type https://slsa.dev/provenance/v1

gh attestation verify ./claw-x86_64-unknown-linux-gnu.tar.xz \
  --repo Shree-git/claw-vcs \
  --predicate-type https://spdx.dev/Document/v2.3
```

## SBOM

When a release includes SPDX or CycloneDX SBOMs:

```bash
jq '.name, .packages | length' claw-vX.Y.Z.sbom.spdx.json
```

Use the SBOM to review dependency inventory and compare it with the release commit.

Each release also includes `claw-vX.Y.Z.release-metadata.json`, which records
the source commit, workflow run URL, runner details, Rust toolchain, target
triple, and Cargo feature metadata used for the release.

## Unix Release-Channel Helper

On Linux or macOS, this helper verifies the release target commit, host archive,
`sha256.sum`, Cosign signatures, GitHub artifact attestations, SBOM readability,
release metadata, the shell installer, and the tagged `cargo install --git` path. Set
`CLAW_RELEASE_VERIFY_REPORT` to keep a machine-readable JSON report with checked
assets, channels, versions, and SHA-256 digests. The report schema is documented
in [Release Channel Report](../reference/release-channel-report.md):

```bash
CLAW_RELEASE_VERIFY_REPORT=release-verification/vX.Y.Z-unix.json scripts/verify-release-channel.sh vX.Y.Z
```

## Install Smoke Test

After installation:

```bash
claw --version
claw doctor
mkdir -p /tmp/claw-demo
cd /tmp/claw-demo
claw init
claw status
```

## If Verification Fails

- Do not run the binary.
- Delete the artifact and re-download from the GitHub Release.
- Confirm you are using the expected repository and tag.
- Open a security advisory if the signature, checksum, or attestation still does not match.
