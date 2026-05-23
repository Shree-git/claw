#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/verify-release-channel.sh <release-tag>

Verifies the release channels that can be checked from the current Unix host:
  - GitHub release archive for this OS/architecture
  - checksums, Cosign signatures, GitHub attestations, and SBOM readability
  - shell installer inside an isolated temporary HOME
  - cargo install from Git for the exact release tag

Set CLAW_VERIFY_HOMEBREW=1 on macOS to also verify the Homebrew tap. Windows
PowerShell installer and MSI checks run in .github/workflows/release-channel-smoke.yml.

Environment:
  CLAW_RELEASE_REPO              GitHub repo to verify (default: Shree-git/claw-vcs)
  CLAW_RELEASE_VERIFY_WORKDIR    Existing work directory to reuse
  CLAW_RELEASE_VERIFY_REPORT     Optional JSON report path to write on pass/fail
  CLAW_SKIP_CARGO_INSTALL=1      Skip the cargo install --git check
  CLAW_KEEP_RELEASE_VERIFY=1     Keep the temporary work directory

Required tools: gh, jq, cosign, tar, shasum, and cargo unless
CLAW_SKIP_CARGO_INSTALL=1 is set.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

tag="${1:-}"
if [[ -z "$tag" ]]; then
  usage >&2
  exit 2
fi

repo="${CLAW_RELEASE_REPO:-Shree-git/claw-vcs}"
workdir="${CLAW_RELEASE_VERIFY_WORKDIR:-$(mktemp -d)}"
cleanup_workdir=0
if [[ "${CLAW_KEEP_RELEASE_VERIFY:-0}" != "1" && -z "${CLAW_RELEASE_VERIFY_WORKDIR:-}" ]]; then
  cleanup_workdir=1
fi

require() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 127
  fi
}

expected_version="${tag#v}"
sbom="claw-${tag}.sbom.spdx.json"
metadata="claw-${tag}.release-metadata.json"
report_path="${CLAW_RELEASE_VERIFY_REPORT:-}"
report_written=0
failure_command=""
failure_line=""

assets="$workdir/assets"
archive_dir="$workdir/archive"
installer_home="$workdir/installer-home"
cargo_root="$workdir/cargo-install"
report_checks="$workdir/report-checks.jsonl"

mkdir -p "$assets" "$archive_dir" "$installer_home"
: > "$report_checks"

record_check() {
  local channel="$1"
  local name="$2"
  local status="$3"
  local details="${4:-}"

  if [[ -z "$details" ]]; then
    details="{}"
  fi

  jq -cn \
    --arg channel "$channel" \
    --arg name "$name" \
    --arg status "$status" \
    --argjson details "$details" \
    '{channel: $channel, name: $name, status: $status, details: $details}' \
    >> "$report_checks"
}

write_report() {
  local exit_status="${1:-0}"

  if [[ -z "$report_path" ]]; then
    return 0
  fi

  mkdir -p "$(dirname "$report_path")"
  jq -s \
    --arg repo "$repo" \
    --arg tag "$tag" \
    --arg os "$(uname -s)" \
    --arg arch "$(uname -m)" \
    --arg expectedVersion "$expected_version" \
    --arg releaseTarget "${release_target:-}" \
    --arg tagCommit "${tag_commit:-}" \
    --arg exitStatus "$exit_status" \
    --arg failureCommand "$failure_command" \
    --arg failureLine "$failure_line" \
    '{
      schemaVersion: 1,
      action: "release_channel.verify",
      generatedAt: now | todateiso8601,
      ok: ($exitStatus == "0"),
      exitStatus: ($exitStatus | tonumber),
      failureCommand: (if $failureCommand == "" then null else $failureCommand end),
      failureLine: (if $failureLine == "" then null else ($failureLine | tonumber) end),
      repo: $repo,
      tag: $tag,
      os: $os,
      arch: $arch,
      expectedVersion: $expectedVersion,
      releaseTarget: $releaseTarget,
      tagCommit: $tagCommit,
      checks: .
    }' "$report_checks" > "$report_path"
}

capture_failure() {
  local command="$1"
  local line="$2"

  if [[ -z "$failure_command" ]]; then
    failure_command="$command"
    failure_line="$line"
  fi
}

finish() {
  local status=$?

  if [[ "$report_written" != "1" && -n "$report_path" ]]; then
    write_report "$status" || true
    if [[ "$status" -ne 0 && -f "$report_path" ]]; then
      echo "Wrote failed release-channel verification report: $report_path" >&2
    fi
  fi

  if [[ "$cleanup_workdir" == "1" ]]; then
    rm -rf "$workdir"
  fi

  exit "$status"
}

trap 'capture_failure "$BASH_COMMAND" "$LINENO"' ERR
trap finish EXIT

require gh
require jq
require cosign
require git
require tar
require shasum

case "$(uname -s):$(uname -m)" in
  Darwin:arm64) archive="claw-vcs-aarch64-apple-darwin.tar.xz" ;;
  Darwin:x86_64) archive="claw-vcs-x86_64-apple-darwin.tar.xz" ;;
  Linux:x86_64) archive="claw-vcs-x86_64-unknown-linux-gnu.tar.xz" ;;
  Linux:aarch64 | Linux:arm64) archive="claw-vcs-aarch64-unknown-linux-gnu.tar.xz" ;;
  *)
    echo "unsupported host for archive verification: $(uname -s) $(uname -m)" >&2
    exit 2
    ;;
esac

echo "Verifying $repo release $tag in $workdir"

release_json="$workdir/release.json"
gh release view "$tag" --repo "$repo" --json tagName,targetCommitish,isDraft,isPrerelease > "$release_json"

release_tag="$(jq -r '.tagName' "$release_json")"
release_target="$(jq -r '.targetCommitish' "$release_json")"
release_is_draft="$(jq -r '.isDraft' "$release_json")"

if [[ "$release_tag" != "$tag" ]]; then
  echo "release metadata tag mismatch: expected $tag, got $release_tag" >&2
  exit 1
fi

if [[ "$release_is_draft" == "true" ]]; then
  echo "release $tag is still a draft; publish before release-channel verification" >&2
  exit 1
fi

tag_refs="$(git ls-remote --tags "https://github.com/${repo}.git" "refs/tags/${tag}" "refs/tags/${tag}^{}")"
tag_commit="$(printf '%s\n' "$tag_refs" | awk -v ref="refs/tags/${tag}^{}" '$2 == ref { print $1; found = 1 } END { if (!found) exit 1 }' || true)"
if [[ -z "$tag_commit" ]]; then
  tag_commit="$(printf '%s\n' "$tag_refs" | awk -v ref="refs/tags/${tag}" '$2 == ref { print $1; exit }')"
fi

if [[ -z "$tag_commit" ]]; then
  echo "could not resolve release tag $tag in $repo" >&2
  exit 1
fi

if [[ "$release_target" != "$tag_commit" ]]; then
  echo "release target mismatch for $tag" >&2
  echo "release targetCommitish: $release_target" >&2
  echo "tag commit:             $tag_commit" >&2
  exit 1
fi

record_check "release" "metadata" "pass" "$(
  jq -cn \
    --arg target "$release_target" \
    --arg commit "$tag_commit" \
    --argjson draft "$release_is_draft" \
    --argjson prerelease "$(jq -r '.isPrerelease' "$release_json")" \
    '{targetCommitish: $target, tagCommit: $commit, draft: $draft, prerelease: $prerelease}'
)"

gh release download "$tag" --repo "$repo" \
  --clobber \
  --pattern "$archive" \
  --pattern "$archive.sigstore.json" \
  --pattern "sha256.sum" \
  --pattern "sha256.sum.sigstore.json" \
  --pattern "claw-vcs-installer.sh" \
  --pattern "claw-vcs-installer.sh.sigstore.json" \
  --pattern "$sbom" \
  --pattern "$sbom.sigstore.json" \
  --pattern "$metadata" \
  --pattern "$metadata.sigstore.json" \
  --dir "$assets"

require_asset() {
  local file="$1"

  if [[ ! -f "$assets/$file" ]]; then
    echo "missing release asset: $file" >&2
    exit 1
  fi
}

require_signed_asset() {
  local file="$1"

  require_asset "$file"
  require_asset "$file.sigstore.json"
}

verify_cosign_blob() {
  local file="$1"
  local repo_identity_pattern="$repo"

  if [[ "$repo" == "Shree-git/claw" || "$repo" == "Shree-git/claw-vcs" ]]; then
    repo_identity_pattern="Shree-git/(claw|claw-vcs)"
  fi

  cosign verify-blob \
    --bundle "$assets/$file.sigstore.json" \
    --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
    --certificate-identity-regexp "^https://github.com/${repo_identity_pattern}/\\.github/workflows/release\\.yml@refs/tags/${tag}$" \
    "$assets/$file"
}

verify_attestation() {
  local file="$1"

  gh attestation verify "$assets/$file" --repo "$repo" \
    --source-ref "refs/tags/${tag}" \
    --source-digest "$tag_commit" \
    --signer-workflow "${repo}/.github/workflows/release.yml" \
    --predicate-type "https://slsa.dev/provenance/v1" \
    --deny-self-hosted-runners
}

verify_sbom_attestation() {
  local file="$1"

  gh attestation verify "$assets/$file" --repo "$repo" \
    --source-ref "refs/tags/${tag}" \
    --source-digest "$tag_commit" \
    --signer-workflow "${repo}/.github/workflows/release.yml" \
    --predicate-type "https://spdx.dev/Document/v2.3" \
    --deny-self-hosted-runners
}

for signed_asset in "$archive" "sha256.sum" "claw-vcs-installer.sh" "$sbom" "$metadata"; do
  require_signed_asset "$signed_asset"
  verify_cosign_blob "$signed_asset"
  record_check "provenance" "cosign:$signed_asset" "pass" "$(jq -cn --arg asset "$signed_asset" '{asset: $asset}')"
  verify_attestation "$signed_asset"
  record_check "provenance" "attestation:$signed_asset" "pass" "$(jq -cn --arg asset "$signed_asset" '{asset: $asset}')"
  verify_sbom_attestation "$signed_asset"
  record_check "sbom" "attestation:$signed_asset" "pass" "$(jq -cn --arg asset "$signed_asset" '{asset: $asset}')"
done

jq -e --arg tag "$tag" --arg commit "$tag_commit" '
  .schemaVersion == 1
  and .tag == $tag
  and .commit == $commit
  and (.run.url | startswith("https://github.com/"))
  and (.runner.os | type == "string")
  and (.toolchain.rustc | startswith("rustc "))
  and (.toolchain.cargo | startswith("cargo "))
  and (.build.defaultFeatures == true)
  and (.build.explicitFeatures | type == "array")
  and (.build.declaredFeatures | type == "object")
' "$assets/$metadata" >/dev/null
record_check "release" "metadata-asset:$metadata" "pass" "$(
  jq -cn \
    --arg asset "$metadata" \
    --arg tag "$tag" \
    --arg commit "$tag_commit" \
    '{asset: $asset, tag: $tag, commit: $commit}'
)"

jq -e '
  .spdxVersion
  and (.SPDXID == "SPDXRef-DOCUMENT")
  and (.packages | type == "array")
  and (.packages | length > 0)
' "$assets/$sbom" >/dev/null
sbom_package_count="$(jq '.packages | length' "$assets/$sbom")"
record_check "sbom" "readable:$sbom" "pass" "$(
  jq -cn \
    --arg asset "$sbom" \
    --argjson packageCount "$sbom_package_count" \
    '{asset: $asset, packageCount: $packageCount}'
)"

verify_sha256_entry() {
  local file="$1"
  local sums="$assets/sha256.sum"
  local expected
  local actual

  expected="$(
    awk -v file="$file" '
      {
        candidate = $2
        sub(/^\*/, "", candidate)
        sub(/^\.\//, "", candidate)
        if (candidate == file) {
          print $1
          found = 1
          exit
        }
      }
      END {
        if (!found) {
          exit 1
        }
      }
    ' "$sums" || true
  )"
  if [[ -z "$expected" ]]; then
    echo "missing checksum entry for $file in sha256.sum" >&2
    exit 1
  fi

  actual="$(shasum -a 256 "$assets/$file" | awk '{ print $1 }')"
  if [[ "$actual" != "$expected" ]]; then
    echo "checksum mismatch for $file" >&2
    echo "expected: $expected" >&2
    echo "actual:   $actual" >&2
    exit 1
  fi
  record_check "checksum" "sha256:$file" "pass" "$(
    jq -cn \
      --arg asset "$file" \
      --arg sha256 "$actual" \
      '{asset: $asset, sha256: $sha256}'
  )"
}

verify_sha256_entry "$archive"
verify_sha256_entry "$sbom"
verify_sha256_entry "$metadata"

smoke_repo() {
  local binary="$1"
  local repo_dir="$2"
  local channel="$3"
  local actual_version

  actual_version="$("$binary" --version | awk '{ print $2 }')"
  if [[ "$actual_version" != "$expected_version" ]]; then
    echo "version mismatch for $binary: expected $expected_version, got ${actual_version:-<empty>}" >&2
    exit 1
  fi
  "$binary" doctor
  mkdir -p "$repo_dir"
  (
    cd "$repo_dir"
    "$binary" init
    "$binary" status
    "$binary" doctor
  )
  record_check "$channel" "binary-smoke" "pass" "$(
    jq -cn \
      --arg binary "$binary" \
      --arg version "$actual_version" \
      '{binary: $binary, version: $version}'
  )"
}

tar -xJf "$assets/$archive" -C "$archive_dir"
archive_binary="$(find "$archive_dir" -type f -name claw | head -n 1)"
if [[ -z "$archive_binary" ]]; then
  echo "archive did not contain a claw binary" >&2
  exit 1
fi
smoke_repo "$archive_binary" "$workdir/archive-repo" "archive"

HOME="$installer_home" bash "$assets/claw-vcs-installer.sh"
installer_binary=""
for candidate in "$installer_home/.local/bin/claw" "$installer_home/.cargo/bin/claw"; do
  if [[ -x "$candidate" ]]; then
    installer_binary="$candidate"
    break
  fi
done
if [[ -z "$installer_binary" ]]; then
  echo "installer did not create a claw binary in the isolated HOME" >&2
  exit 1
fi
smoke_repo "$installer_binary" "$workdir/installer-repo" "shell-installer"

if [[ "${CLAW_SKIP_CARGO_INSTALL:-0}" != "1" ]]; then
  require cargo
  cargo install --git "https://github.com/${repo}.git" --tag "$tag" claw-vcs --locked --root "$cargo_root"
  smoke_repo "$cargo_root/bin/claw" "$workdir/cargo-repo" "cargo-install-git"
else
  echo "Skipping cargo install --git check because CLAW_SKIP_CARGO_INSTALL=1"
  record_check "cargo-install-git" "binary-smoke" "skipped" '{"reason":"CLAW_SKIP_CARGO_INSTALL=1"}'
fi

if [[ "$(uname -s)" == "Darwin" && "${CLAW_VERIFY_HOMEBREW:-0}" == "1" ]]; then
  require brew
  brew tap shree-git/tap
  brew install shree-git/tap/claw
  homebrew_binary="$(brew list shree-git/tap/claw | awk '/\/bin\/claw$/ { print; exit }')"
  if [[ -z "$homebrew_binary" ]]; then
    homebrew_prefix="$(brew --prefix shree-git/tap/claw)"
    homebrew_binary="$homebrew_prefix/bin/claw"
  fi
  if [[ ! -x "$homebrew_binary" ]]; then
    echo "Homebrew-installed claw binary not found for shree-git/tap/claw" >&2
    exit 1
  fi
  smoke_repo "$homebrew_binary" "$workdir/homebrew-repo" "homebrew"
else
  echo "Skipping Homebrew check; set CLAW_VERIFY_HOMEBREW=1 on macOS to enable it."
  record_check "homebrew" "binary-smoke" "skipped" '{"reason":"CLAW_VERIFY_HOMEBREW not enabled or host is not Darwin"}'
fi

write_report 0
report_written=1
echo "Release-channel verification passed for $repo $tag"
if [[ -n "$report_path" ]]; then
  echo "Wrote release-channel verification report: $report_path"
fi
