"""Python helpers for agents that drive the Claw CLI."""

from __future__ import annotations

from dataclasses import dataclass, field
import json
import subprocess
from pathlib import Path
from typing import Any, Mapping, Sequence


class ClawCommandError(RuntimeError):
    """Raised when the `claw` process exits unsuccessfully."""

    def __init__(self, message: str, *, status: int, stdout: str, stderr: str) -> None:
        super().__init__(message)
        self.status = status
        self.stdout = stdout
        self.stderr = stderr


@dataclass(frozen=True)
class ClawClient:
    """Small CLI-backed client for Claw agent integrations."""

    binary: str = "claw"
    cwd: str | Path | None = None
    env: Mapping[str, str] | None = None

    def output(self, args: Sequence[str]) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(
            [self.binary, *args],
            cwd=self.cwd,
            env=dict(self.env) if self.env is not None else None,
            text=True,
            capture_output=True,
            check=False,
        )
        if result.returncode != 0:
            raise ClawCommandError(
                f"claw exited with status {result.returncode}",
                status=result.returncode,
                stdout=result.stdout,
                stderr=result.stderr,
            )
        return result

    def json(self, args: Sequence[str]) -> Any:
        return json.loads(self.output(args).stdout)

    def status(self) -> Any:
        return self.json(["status", "--json"])

    def list_intents(self) -> Any:
        return self.json(["intent", "--json", "list"])

    def intent_graph(self) -> Any:
        return self.json(["intent", "--json", "graph"])

    def create_intent(
        self,
        title: str,
        goal: str = "",
        acceptance_tests: Sequence[str] = (),
    ) -> Any:
        return self.create_intent_with(
            IntentCreate(title=title, goal=goal, acceptance_tests=list(acceptance_tests))
        )

    def create_intent_with(self, input: "IntentCreate") -> Any:
        args = ["intent", "--json", "create", "--title", input.title, "--goal", input.goal]
        _extend_repeated(args, "--acceptance-test", input.acceptance_tests)
        return self.json(args)

    def create_change(self, intent_id: str) -> Any:
        return self.json(["change", "--json", "create", "--intent", intent_id])

    def run_acceptance(self, input: "AcceptanceRun") -> Any:
        args = [
            "intent",
            "--json",
            "run-acceptance",
            input.intent_id,
            "--timeout-ms",
            str(input.timeout_ms),
        ]
        if input.keep_going:
            args.append("--keep-going")
        return self.json(args)

    def query_evidence(self, query: str | "EvidenceQuery") -> Any:
        input = EvidenceQuery(query=query) if isinstance(query, str) else query
        args = ["evidence", "--json", "query", input.query]
        _extend_optional(args, "--revision", input.revision)
        _extend_optional(args, "--capsule", input.capsule)
        if input.limit is not None:
            args.extend(["--limit", str(input.limit)])
        return self.json(args)

    def doctor(self, input: "DoctorRequest" | None = None) -> Any:
        input = input or DoctorRequest()
        args = ["doctor", "--json"]
        if input.deep:
            args.append("--deep")
        if input.strict:
            args.append("--strict")
        return self.json(args)

    def doctor_deep(self) -> Any:
        return self.doctor(DoctorRequest(deep=True))

    def repair_plan(self) -> Any:
        return self.json(["repair", "--json", "plan"])

    def agent_audit(self, input: "AgentAuditRequest" | None = None) -> Any:
        input = input or AgentAuditRequest()
        args = ["agent", "--json", "audit"]
        _extend_optional(args, "--status", input.status)
        _extend_optional(args, "--risk", input.risk)
        if input.action_required:
            args.append("--action-required")
        return self.json(args)

    def agent_bulk(self, input: "AgentBulkRequest") -> Any:
        args = ["agent", "--json", "bulk", "--file", input.file]
        if input.dry_run:
            args.append("--dry-run")
        return self.json(args)

    def story_export(self, input: "StoryExportRequest") -> Any:
        return self.json(["story", "export", "--intent", input.intent, "--format", "json"])

    def timeline_allowed(self, input: "TimelineAllowedRequest") -> Any:
        args = ["timeline", "--json", "allowed", "--revision", input.revision]
        _extend_repeated(args, "--policy", input.policies)
        _extend_repeated(args, "--signer-agent", input.signer_agents)
        _extend_repeated(args, "--signer-key", input.signer_keys)
        _extend_repeated(args, "--path", input.paths)
        return self.json(args)

    def patch_workbench(self, input: "PatchWorkbenchRequest") -> Any:
        return self.json([
            "patch",
            "--json",
            "workbench",
            "--left",
            input.left,
            "--right",
            input.right,
        ])

    def policy_simulate(self, input: "PolicySimulateRequest") -> Any:
        args = ["policy", "simulate"]
        if input.policy_file is not None:
            args.extend(["--policy-file", input.policy_file])
        elif input.policy_id is not None:
            args.append(input.policy_id)
        args.extend(["--revision", input.revision])
        _extend_optional(args, "--capsule", input.capsule)
        _extend_repeated(args, "--signer-agent", input.signer_agents)
        _extend_repeated(args, "--signer-key", input.signer_keys)
        _extend_repeated(args, "--path", input.paths)
        _extend_optional(args, "--trust-score", input.trust_score)
        args.append("--json")
        return self.json(args)

    def review(self, input: "ReviewRequest" | None = None) -> Any:
        input = input or ReviewRequest()
        args = ["review", "--json"]
        _extend_optional(args, "--intent", input.intent)
        _extend_optional(args, "--change", input.change)
        _extend_optional(args, "--capsule", input.capsule)
        return self.json(args)

    def inspect_capsule(self, target: str) -> Any:
        return self.json(["capsule", "--json", "inspect", target])

    def trust_receipt(self, input: "TrustReceiptRequest") -> Any:
        args = ["trust", "--json", "receipt", "--revision", input.revision]
        _extend_optional(args, "--capsule", input.capsule)
        _extend_repeated(args, "--policy", input.policies)
        _extend_repeated(args, "--signer-agent", input.signer_agents)
        _extend_repeated(args, "--signer-key", input.signer_keys)
        _extend_repeated(args, "--path", input.paths)
        _extend_optional(args, "--trust-score", input.trust_score)
        return self.json(args)

    def ship(self, input: "ShipRequest") -> Any:
        args = ["ship", "--json", "--intent", input.intent]
        _extend_optional(args, "--revision-ref", input.revision_ref)
        _extend_optional(args, "--agent", input.agent)
        _extend_repeated(args, "--evidence", input.evidence)
        _extend_optional(args, "--evidence-command", input.evidence_command)
        _extend_optional(args, "--runner", input.runner)
        _extend_optional(args, "--environment-digest", input.environment_digest)
        _extend_optional(args, "--log-digest", input.log_digest)
        _extend_optional(args, "--artifact-digest", input.artifact_digest)
        if input.evidence_expires_in_ms is not None:
            args.extend(["--evidence-expires-in-ms", str(input.evidence_expires_in_ms)])
        _extend_optional(args, "--private-file", input.private_file)
        _extend_repeated(args, "--recipient-key", input.recipient_keys)
        _extend_repeated(args, "--co-sign", input.co_signers)
        return self.json(args)

    def provenance_replay(self, input: "ProvenanceReplayRequest") -> Any:
        args = ["provenance", "--json", "replay", "--revision", input.revision]
        _extend_optional(args, "--capsule", input.capsule)
        _extend_repeated(args, "--evidence", input.evidence_names)
        _extend_optional(args, "--agent", input.agent)
        if input.timeout_ms is not None:
            args.extend(["--timeout-ms", str(input.timeout_ms)])
        if input.keep_going:
            args.append("--keep-going")
        if input.in_place:
            args.append("--in-place")
        if input.dry_run:
            args.append("--dry-run")
        return self.json(args)

    def attach_attestation(self, input: "AttestationRequest") -> Any:
        args = [
            "provenance",
            "--json",
            "attach-attestation",
            "--revision",
            input.revision,
            "--file",
            input.file,
        ]
        _extend_optional(args, "--capsule", input.capsule)
        _extend_optional(args, "--agent", input.agent)
        _extend_optional(args, "--subject-name", input.subject_name)
        _extend_optional(args, "--subject-digest", input.subject_digest)
        _extend_optional(args, "--builder-id", input.builder_id)
        _extend_optional(args, "--build-type", input.build_type)
        if input.dry_run:
            args.append("--dry-run")
        return self.json(args)


@dataclass(frozen=True)
class IntentCreate:
    """Typed input for creating an intent."""

    title: str
    goal: str = ""
    acceptance_tests: Sequence[str] = field(default_factory=tuple)


@dataclass(frozen=True)
class AcceptanceRun:
    """Typed input for running executable intent acceptance tests."""

    intent_id: str
    timeout_ms: int = 300_000
    keep_going: bool = False


@dataclass(frozen=True)
class EvidenceQuery:
    """Typed evidence query with optional capsule/revision filters."""

    query: str
    revision: str | None = None
    capsule: str | None = None
    limit: int | None = None


@dataclass(frozen=True)
class DoctorRequest:
    """Typed request for `claw doctor --json`."""

    deep: bool = False
    strict: bool = False


@dataclass(frozen=True)
class AgentAuditRequest:
    """Typed request for filtered `claw agent audit --json`."""

    status: str | None = None
    risk: str | None = None
    action_required: bool = False


@dataclass(frozen=True)
class AgentBulkRequest:
    """Typed request for `claw agent bulk --json`."""

    file: str
    dry_run: bool = False


@dataclass(frozen=True)
class StoryExportRequest:
    """Typed request for `claw story export --format json`."""

    intent: str


@dataclass(frozen=True)
class TimelineAllowedRequest:
    """Typed request for `claw timeline --json allowed`."""

    revision: str
    policies: Sequence[str] = field(default_factory=tuple)
    signer_agents: Sequence[str] = field(default_factory=tuple)
    signer_keys: Sequence[str] = field(default_factory=tuple)
    paths: Sequence[str] = field(default_factory=tuple)


@dataclass(frozen=True)
class PatchWorkbenchRequest:
    """Typed request for `claw patch --json workbench`."""

    left: str
    right: str


@dataclass(frozen=True)
class PolicySimulateRequest:
    """Typed request for `claw policy simulate`."""

    revision: str
    policy_id: str | None = None
    policy_file: str | None = None
    capsule: str | None = None
    signer_agents: Sequence[str] = field(default_factory=tuple)
    signer_keys: Sequence[str] = field(default_factory=tuple)
    paths: Sequence[str] = field(default_factory=tuple)
    trust_score: str | None = None


@dataclass(frozen=True)
class ReviewRequest:
    """Typed review selector."""

    intent: str | None = None
    change: str | None = None
    capsule: str | None = None


@dataclass(frozen=True)
class TrustReceiptRequest:
    """Typed trust receipt request."""

    revision: str
    capsule: str | None = None
    policies: Sequence[str] = field(default_factory=tuple)
    signer_agents: Sequence[str] = field(default_factory=tuple)
    signer_keys: Sequence[str] = field(default_factory=tuple)
    paths: Sequence[str] = field(default_factory=tuple)
    trust_score: str | None = None


@dataclass(frozen=True)
class ShipRequest:
    """Typed request for `claw ship --json`."""

    intent: str
    revision_ref: str | None = None
    agent: str | None = None
    evidence: Sequence[str] = field(default_factory=tuple)
    evidence_command: str | None = None
    runner: str | None = None
    environment_digest: str | None = None
    log_digest: str | None = None
    artifact_digest: str | None = None
    evidence_expires_in_ms: int | None = None
    private_file: str | None = None
    recipient_keys: Sequence[str] = field(default_factory=tuple)
    co_signers: Sequence[str] = field(default_factory=tuple)


@dataclass(frozen=True)
class ProvenanceReplayRequest:
    """Typed request for provenance replay."""

    revision: str
    capsule: str | None = None
    evidence_names: Sequence[str] = field(default_factory=tuple)
    agent: str | None = None
    timeout_ms: int | None = None
    keep_going: bool = False
    in_place: bool = False
    dry_run: bool = False


@dataclass(frozen=True)
class AttestationRequest:
    """Typed request for attaching a SLSA/in-toto attestation."""

    revision: str
    file: str
    capsule: str | None = None
    agent: str | None = None
    subject_name: str | None = None
    subject_digest: str | None = None
    builder_id: str | None = None
    build_type: str | None = None
    dry_run: bool = False


def mcp_server_command(binary: str = "claw", *, allow_write: bool = False) -> list[str]:
    """Return the command used to launch the Claw MCP stdio server."""

    command = [binary, "mcp", "serve"]
    if allow_write:
        command.append("--allow-write")
    return command


def _extend_optional(args: list[str], flag: str, value: str | None) -> None:
    if value is not None:
        args.extend([flag, value])


def _extend_repeated(args: list[str], flag: str, values: Sequence[str]) -> None:
    for value in values:
        args.extend([flag, value])


__all__ = [
    "AcceptanceRun",
    "AgentAuditRequest",
    "AgentBulkRequest",
    "AttestationRequest",
    "ClawClient",
    "ClawCommandError",
    "DoctorRequest",
    "EvidenceQuery",
    "IntentCreate",
    "PatchWorkbenchRequest",
    "PolicySimulateRequest",
    "ProvenanceReplayRequest",
    "ReviewRequest",
    "ShipRequest",
    "StoryExportRequest",
    "TimelineAllowedRequest",
    "TrustReceiptRequest",
    "mcp_server_command",
]
