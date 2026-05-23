#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/verify-repo-health.sh [repo-path]

Runs the machine-readable Claw repository health gate used by production
readiness reviews. The target must already be a Claw repository.

Checks:
  - claw version --json
  - claw doctor --json --strict
  - claw repair --json plan with zero summary.error_count and zero repairable_count

Environment:
  CLAW_HEALTH_CLAW_BIN  Claw binary to run (default: claw)
  CLAW_HEALTH_REPORT    Optional aggregate JSON report path to write on pass/fail
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd "$script_dir/.." && pwd -P)"
repo_path="${1:-.}"
claw_bin="${CLAW_HEALTH_CLAW_BIN:-claw}"
report_path="${CLAW_HEALTH_REPORT:-}"

case "$repo_path" in
  /*)
    ;;
  *)
    if [[ -d "$repo_path" ]]; then
      repo_path="$(cd "$repo_path" && pwd -P)"
    else
      repo_path="$(pwd -P)/$repo_path"
    fi
    ;;
esac

case "$report_path" in
  "")
    ;;
  /*)
    ;;
  *)
    report_path="$repo_root/$report_path"
    ;;
esac

case "$claw_bin" in
  */*)
    case "$claw_bin" in
      /*)
        ;;
      *)
        claw_bin="$(cd "$(dirname "$claw_bin")" && pwd -P)/$(basename "$claw_bin")"
        ;;
    esac
    ;;
esac

require() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 127
  fi
}

require python3
if [[ "$claw_bin" != */* ]]; then
  require "$claw_bin"
elif [[ ! -x "$claw_bin" ]]; then
  echo "claw binary is not executable: $claw_bin" >&2
  exit 127
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

version_json="$tmpdir/version.json"
doctor_json="$tmpdir/doctor.json"
repair_json="$tmpdir/repair.json"

version_status=0
doctor_status=0
repair_status=0
("$claw_bin" version --json >"$version_json") || version_status=$?
(cd "$repo_path" && "$claw_bin" doctor --json --strict >"$doctor_json") || doctor_status=$?
(cd "$repo_path" && "$claw_bin" repair --json plan >"$repair_json") || repair_status=$?

if ! python3 - "$version_json" "$doctor_json" "$repair_json" "$report_path" "$version_status" "$doctor_status" "$repair_status" "$repo_path" "$claw_bin" <<'PY'
import json
import pathlib
import sys
import time

version_path = pathlib.Path(sys.argv[1])
doctor_path = pathlib.Path(sys.argv[2])
repair_path = pathlib.Path(sys.argv[3])
report_arg = sys.argv[4]
version_status = int(sys.argv[5])
doctor_status = int(sys.argv[6])
repair_status = int(sys.argv[7])
repo_path = sys.argv[8]
claw_bin = sys.argv[9]


def load_json(path):
    if not path.exists():
        return None, {"parse_error": "output file was not created", "raw": ""}
    text = path.read_text(encoding="utf-8")
    try:
        return json.loads(text), None
    except json.JSONDecodeError as exc:
        return None, {"parse_error": str(exc), "raw": text}


failures = []
version, version_error = load_json(version_path)
doctor, doctor_error = load_json(doctor_path)
repair, repair_error = load_json(repair_path)

if version_status != 0:
    failures.append(f"version command exited {version_status}")
if doctor_status != 0:
    failures.append(f"doctor command exited {doctor_status}")
if repair_status != 0:
    failures.append(f"repair plan command exited {repair_status}")
if version_error is not None:
    failures.append("version output was not valid JSON")
if doctor_error is not None:
    failures.append("doctor output was not valid JSON")
if repair_error is not None:
    failures.append("repair plan output was not valid JSON")

if version is not None and (
    version.get("schema_version") != 1 or version.get("action") != "version"
):
    failures.append("version JSON must be schema_version 1 with action version")
if doctor is not None and (
    doctor.get("schema_version") != 1 or doctor.get("action") != "doctor"
):
    failures.append("doctor JSON must be schema_version 1 with action doctor")
if repair is not None and (
    repair.get("schema_version") != 1 or repair.get("action") != "repair.plan"
):
    failures.append("repair plan JSON must be schema_version 1 with action repair.plan")

if repair is not None:
    summary = repair.get("summary") or {}
    error_count = summary.get("error_count")
    repairable_count = repair.get("repairable_count")
    if error_count != 0:
        failures.append(f"repair summary.error_count expected 0, got {error_count!r}")
    if repairable_count != 0:
        failures.append(f"repairable_count expected 0, got {repairable_count!r}")

report = {
    "schema_version": 1,
    "action": "repo_health.verify",
    "generated_at_ms": int(time.time() * 1000),
    "ok": not failures,
    "repo_path": repo_path,
    "claw_bin": claw_bin,
    "failures": failures,
    "checks": {
        "version": {
            "command": "claw version --json",
            "exit_code": version_status,
        },
        "doctor": {
            "command": "claw doctor --json --strict",
            "exit_code": doctor_status,
        },
        "repair_plan": {
            "command": "claw repair --json plan",
            "exit_code": repair_status,
        },
    },
    "version": version if version is not None else version_error,
    "doctor": doctor if doctor is not None else doctor_error,
    "repair_plan": repair if repair is not None else repair_error,
}
if report_arg:
    report_path = pathlib.Path(report_arg)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

if failures:
    for failure in failures:
        print(f"FAIL: {failure}", file=sys.stderr)
    sys.exit(1)
PY
then
  if [[ -n "$report_path" && -f "$report_path" ]]; then
    echo "Wrote failed health report: $report_path" >&2
  fi
  exit 1
fi

echo "Repository health verified for $repo_path"
if [[ -n "$report_path" ]]; then
  echo "Wrote health report: $report_path"
fi
