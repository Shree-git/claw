import { spawn } from "node:child_process";

export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export interface ClawClientOptions {
  binary?: string;
  cwd?: string;
  env?: NodeJS.ProcessEnv;
}

export interface CommandResult {
  status: number;
  stdout: string;
  stderr: string;
}

export interface IntentCreateInput {
  title: string;
  goal?: string;
  acceptanceTests?: string[];
}

export interface AcceptanceRunInput {
  intentId: string;
  timeoutMs?: number;
  keepGoing?: boolean;
}

export interface EvidenceQueryInput {
  query: string;
  revision?: string;
  capsule?: string;
  limit?: number;
}

export interface DoctorInput {
  deep?: boolean;
  strict?: boolean;
}

export interface AgentAuditInput {
  status?: string;
  risk?: string;
  actionRequired?: boolean;
}

export interface AgentBulkInput {
  file: string;
  dryRun?: boolean;
}

export interface StoryExportInput {
  intent: string;
}

export interface TimelineAllowedInput {
  revision: string;
  policies?: string[];
  signerAgents?: string[];
  signerKeys?: string[];
  paths?: string[];
}

export interface PatchWorkbenchInput {
  left: string;
  right: string;
}

export interface PolicySimulateInput {
  policyId?: string;
  policyFile?: string;
  revision: string;
  capsule?: string;
  signerAgents?: string[];
  signerKeys?: string[];
  paths?: string[];
  trustScore?: string;
}

export interface ReviewInput {
  intent?: string;
  change?: string;
  capsule?: string;
}

export interface TrustReceiptInput {
  revision: string;
  capsule?: string;
  policies?: string[];
  signerAgents?: string[];
  signerKeys?: string[];
  paths?: string[];
  trustScore?: string;
}

export interface ShipInput {
  intent: string;
  revisionRef?: string;
  agent?: string;
  evidence?: string[];
  evidenceCommand?: string;
  runner?: string;
  environmentDigest?: string;
  logDigest?: string;
  artifactDigest?: string;
  evidenceExpiresInMs?: number;
  privateFile?: string;
  recipientKeys?: string[];
  coSigners?: string[];
}

export interface ProvenanceReplayInput {
  revision: string;
  capsule?: string;
  evidenceNames?: string[];
  agent?: string;
  timeoutMs?: number;
  keepGoing?: boolean;
  inPlace?: boolean;
  dryRun?: boolean;
}

export interface AttestationInput {
  revision: string;
  capsule?: string;
  file: string;
  agent?: string;
  subjectName?: string;
  subjectDigest?: string;
  builderId?: string;
  buildType?: string;
  dryRun?: boolean;
}

export class ClawCommandError extends Error {
  status: number;
  stdout: string;
  stderr: string;

  constructor(message: string, result: CommandResult) {
    super(message);
    this.name = "ClawCommandError";
    this.status = result.status;
    this.stdout = result.stdout;
    this.stderr = result.stderr;
  }
}

export class ClawClient {
  readonly binary: string;
  readonly cwd?: string;
  readonly env?: NodeJS.ProcessEnv;

  constructor(options: ClawClientOptions = {}) {
    this.binary = options.binary ?? "claw";
    this.cwd = options.cwd;
    this.env = options.env;
  }

  async json(args: string[]): Promise<JsonValue> {
    const result = await this.output(args);
    return JSON.parse(result.stdout) as JsonValue;
  }

  async output(args: string[]): Promise<CommandResult> {
    return new Promise((resolve, reject) => {
      const child = spawn(this.binary, args, {
        cwd: this.cwd,
        env: this.env,
        stdio: ["ignore", "pipe", "pipe"]
      });
      let stdout = "";
      let stderr = "";
      child.stdout.setEncoding("utf8");
      child.stderr.setEncoding("utf8");
      child.stdout.on("data", chunk => {
        stdout += chunk;
      });
      child.stderr.on("data", chunk => {
        stderr += chunk;
      });
      child.on("error", reject);
      child.on("close", status => {
        const result = { status: status ?? -1, stdout, stderr };
        if (result.status === 0) {
          resolve(result);
        } else {
          reject(new ClawCommandError(`claw exited with status ${result.status}`, result));
        }
      });
    });
  }

  status(): Promise<JsonValue> {
    return this.json(["status", "--json"]);
  }

  listIntents(): Promise<JsonValue> {
    return this.json(["intent", "--json", "list"]);
  }

  intentGraph(): Promise<JsonValue> {
    return this.json(["intent", "--json", "graph"]);
  }

  createIntent(input: IntentCreateInput): Promise<JsonValue> {
    const args = ["intent", "--json", "create", "--title", input.title, "--goal", input.goal ?? ""];
    for (const test of input.acceptanceTests ?? []) {
      args.push("--acceptance-test", test);
    }
    return this.json(args);
  }

  createChange(intentId: string): Promise<JsonValue> {
    return this.json(["change", "--json", "create", "--intent", intentId]);
  }

  runAcceptance(input: AcceptanceRunInput): Promise<JsonValue> {
    const args = [
      "intent",
      "--json",
      "run-acceptance",
      input.intentId,
      "--timeout-ms",
      String(input.timeoutMs ?? 300000)
    ];
    if (input.keepGoing) {
      args.push("--keep-going");
    }
    return this.json(args);
  }

  queryEvidence(query: string | EvidenceQueryInput): Promise<JsonValue> {
    const input = typeof query === "string" ? { query } : query;
    const args = ["evidence", "--json", "query", input.query];
    pushOptional(args, "--revision", input.revision);
    pushOptional(args, "--capsule", input.capsule);
    if (input.limit !== undefined) {
      args.push("--limit", String(input.limit));
    }
    return this.json(args);
  }

  doctor(input: DoctorInput = {}): Promise<JsonValue> {
    const args = ["doctor", "--json"];
    if (input.deep) {
      args.push("--deep");
    }
    if (input.strict) {
      args.push("--strict");
    }
    return this.json(args);
  }

  doctorDeep(): Promise<JsonValue> {
    return this.doctor({ deep: true });
  }

  repairPlan(): Promise<JsonValue> {
    return this.json(["repair", "--json", "plan"]);
  }

  agentAudit(input: AgentAuditInput = {}): Promise<JsonValue> {
    const args = ["agent", "--json", "audit"];
    pushOptional(args, "--status", input.status);
    pushOptional(args, "--risk", input.risk);
    if (input.actionRequired) {
      args.push("--action-required");
    }
    return this.json(args);
  }

  agentBulk(input: AgentBulkInput): Promise<JsonValue> {
    const args = ["agent", "--json", "bulk", "--file", input.file];
    if (input.dryRun) {
      args.push("--dry-run");
    }
    return this.json(args);
  }

  storyExport(input: StoryExportInput): Promise<JsonValue> {
    return this.json(["story", "export", "--intent", input.intent, "--format", "json"]);
  }

  timelineAllowed(input: TimelineAllowedInput): Promise<JsonValue> {
    const args = ["timeline", "--json", "allowed", "--revision", input.revision];
    pushRepeated(args, "--policy", input.policies);
    pushRepeated(args, "--signer-agent", input.signerAgents);
    pushRepeated(args, "--signer-key", input.signerKeys);
    pushRepeated(args, "--path", input.paths);
    return this.json(args);
  }

  patchWorkbench(input: PatchWorkbenchInput): Promise<JsonValue> {
    return this.json([
      "patch",
      "--json",
      "workbench",
      "--left",
      input.left,
      "--right",
      input.right
    ]);
  }

  policySimulate(input: PolicySimulateInput): Promise<JsonValue> {
    const args = ["policy", "simulate"];
    if (input.policyFile !== undefined) {
      args.push("--policy-file", input.policyFile);
    } else if (input.policyId !== undefined) {
      args.push(input.policyId);
    }
    args.push("--revision", input.revision);
    pushOptional(args, "--capsule", input.capsule);
    pushRepeated(args, "--signer-agent", input.signerAgents);
    pushRepeated(args, "--signer-key", input.signerKeys);
    pushRepeated(args, "--path", input.paths);
    pushOptional(args, "--trust-score", input.trustScore);
    args.push("--json");
    return this.json(args);
  }

  review(input: ReviewInput = {}): Promise<JsonValue> {
    const args = ["review", "--json"];
    pushOptional(args, "--intent", input.intent);
    pushOptional(args, "--change", input.change);
    pushOptional(args, "--capsule", input.capsule);
    return this.json(args);
  }

  inspectCapsule(target: string): Promise<JsonValue> {
    return this.json(["capsule", "--json", "inspect", target]);
  }

  trustReceipt(input: TrustReceiptInput): Promise<JsonValue> {
    const args = ["trust", "--json", "receipt", "--revision", input.revision];
    pushOptional(args, "--capsule", input.capsule);
    pushRepeated(args, "--policy", input.policies);
    pushRepeated(args, "--signer-agent", input.signerAgents);
    pushRepeated(args, "--signer-key", input.signerKeys);
    pushRepeated(args, "--path", input.paths);
    pushOptional(args, "--trust-score", input.trustScore);
    return this.json(args);
  }

  ship(input: ShipInput): Promise<JsonValue> {
    const args = ["ship", "--json", "--intent", input.intent];
    pushOptional(args, "--revision-ref", input.revisionRef);
    pushOptional(args, "--agent", input.agent);
    pushRepeated(args, "--evidence", input.evidence);
    pushOptional(args, "--evidence-command", input.evidenceCommand);
    pushOptional(args, "--runner", input.runner);
    pushOptional(args, "--environment-digest", input.environmentDigest);
    pushOptional(args, "--log-digest", input.logDigest);
    pushOptional(args, "--artifact-digest", input.artifactDigest);
    if (input.evidenceExpiresInMs !== undefined) {
      args.push("--evidence-expires-in-ms", String(input.evidenceExpiresInMs));
    }
    pushOptional(args, "--private-file", input.privateFile);
    pushRepeated(args, "--recipient-key", input.recipientKeys);
    pushRepeated(args, "--co-sign", input.coSigners);
    return this.json(args);
  }

  provenanceReplay(input: ProvenanceReplayInput): Promise<JsonValue> {
    const args = ["provenance", "--json", "replay", "--revision", input.revision];
    pushOptional(args, "--capsule", input.capsule);
    pushRepeated(args, "--evidence", input.evidenceNames);
    pushOptional(args, "--agent", input.agent);
    if (input.timeoutMs !== undefined) {
      args.push("--timeout-ms", String(input.timeoutMs));
    }
    if (input.keepGoing) {
      args.push("--keep-going");
    }
    if (input.inPlace) {
      args.push("--in-place");
    }
    if (input.dryRun) {
      args.push("--dry-run");
    }
    return this.json(args);
  }

  attachAttestation(input: AttestationInput): Promise<JsonValue> {
    const args = [
      "provenance",
      "--json",
      "attach-attestation",
      "--revision",
      input.revision,
      "--file",
      input.file
    ];
    pushOptional(args, "--capsule", input.capsule);
    pushOptional(args, "--agent", input.agent);
    pushOptional(args, "--subject-name", input.subjectName);
    pushOptional(args, "--subject-digest", input.subjectDigest);
    pushOptional(args, "--builder-id", input.builderId);
    pushOptional(args, "--build-type", input.buildType);
    if (input.dryRun) {
      args.push("--dry-run");
    }
    return this.json(args);
  }
}

function pushOptional(args: string[], flag: string, value: string | undefined): void {
  if (value !== undefined) {
    args.push(flag, value);
  }
}

function pushRepeated(args: string[], flag: string, values: string[] | undefined): void {
  for (const value of values ?? []) {
    args.push(flag, value);
  }
}

export function mcpServerCommand(options: { binary?: string; allowWrite?: boolean } = {}): string[] {
  const args = [options.binary ?? "claw", "mcp", "serve"];
  if (options.allowWrite) {
    args.push("--allow-write");
  }
  return args;
}
