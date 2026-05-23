use clap::Args;

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use claw_core::id::{ChangeId, ObjectId};
use claw_core::object::Object;
use claw_core::types::{
    Capsule, CapsulePublic, ChangeStatus, Evidence, Intent, IntentStatus, Policy, Revision,
};
use claw_crypto::capsule::{append_capsule_signature, build_capsule, build_capsule_for_recipients};
use claw_crypto::recipient::RecipientPublicKey;
use claw_policy::{evaluator::evaluate_policy, PolicyContext};
use claw_store::ClawStore;

use super::agent::{ensure_registered_signing_agent, keypair_for_agent, AgentRegistration};
use crate::config::{find_repo_root, load_or_default_config};

#[derive(Args)]
pub struct ShipArgs {
    /// Output a machine-readable JSON receipt
    #[arg(long)]
    json: bool,
    /// Intent ID to ship
    #[arg(short, long)]
    intent: String,
    /// Revision ref to ship
    #[arg(short, long, default_value = "heads/main")]
    revision_ref: String,
    /// Agent ID
    #[arg(short, long, default_value = "claw")]
    agent: String,
    /// Capsule evidence item in the form `name=status[:duration_ms]`.
    #[arg(long = "evidence")]
    evidence: Vec<String>,
    /// Command that produced the supplied evidence
    #[arg(long = "evidence-command")]
    evidence_command: Option<String>,
    /// Runner identity that produced the supplied evidence
    #[arg(long = "runner")]
    runner_identity: Option<String>,
    /// Environment/toolchain digest for freshness-gated evidence
    #[arg(long = "environment-digest")]
    environment_digest: Option<String>,
    /// Log digest for freshness-gated evidence
    #[arg(long = "log-digest")]
    log_digest: Option<String>,
    /// Artifact digest for freshness-gated evidence
    #[arg(long = "artifact-digest")]
    artifact_digest: Option<String>,
    /// Evidence expiration window in milliseconds
    #[arg(long = "evidence-expires-in-ms")]
    evidence_expires_in_ms: Option<u64>,
    /// Run executable acceptance tests linked to the intent and attach results as capsule evidence.
    #[arg(long = "run-acceptance")]
    run_acceptance: bool,
    /// Stop each linked acceptance command after this many milliseconds.
    #[arg(long = "acceptance-timeout-ms", default_value_t = 300_000)]
    acceptance_timeout_ms: u64,
    /// Continue running remaining acceptance commands after a failure, but still block shipping if any fail.
    #[arg(long = "acceptance-keep-going")]
    acceptance_keep_going: bool,
    /// File containing private capsule metadata to encrypt
    #[arg(long = "private-file")]
    private_file: Option<PathBuf>,
    /// Recipient envelope in the form recipient-id:key-id:hex-x25519-public-key
    #[arg(long = "recipient-key")]
    recipient_keys: Vec<String>,
    /// Add an additional registered agent signature to the capsule
    #[arg(long = "co-sign")]
    co_sign: Vec<String>,
}

fn parse_evidence(items: &[String]) -> anyhow::Result<Vec<Evidence>> {
    let mut out = Vec::with_capacity(items.len());

    for raw in items {
        let (name, status_and_duration) = if let Some(parts) = raw.split_once('=') {
            parts
        } else if let Some(parts) = raw.split_once(':') {
            parts
        } else {
            anyhow::bail!(
                "invalid --evidence '{}'; expected name=status[:duration_ms]",
                raw
            );
        };

        if name.trim().is_empty() {
            anyhow::bail!("invalid --evidence '{}'; name cannot be empty", raw);
        }

        let (status, duration_ms) =
            if let Some((status, duration)) = status_and_duration.split_once(':') {
                let parsed = duration.parse::<u64>().map_err(|_| {
                    anyhow::anyhow!(
                        "invalid --evidence '{}'; duration_ms must be an unsigned integer",
                        raw
                    )
                })?;
                (status, parsed)
            } else {
                (status_and_duration, 0)
            };

        if status.trim().is_empty() {
            anyhow::bail!("invalid --evidence '{}'; status cannot be empty", raw);
        }

        out.push(Evidence {
            name: name.trim().to_string(),
            status: status.trim().to_string(),
            duration_ms,
            artifact_refs: vec![],
            summary: None,
            revision_id: None,
            command: None,
            exit_code: None,
            started_at_ms: None,
            ended_at_ms: None,
            environment_digest: None,
            runner_identity: None,
            log_digest: None,
            artifact_digest: None,
            expires_at_ms: None,
            trust_domain: None,
            signature: None,
        });
    }

    Ok(out)
}

fn parse_recipient_public_keys(items: &[String]) -> anyhow::Result<Vec<RecipientPublicKey>> {
    let mut out = Vec::with_capacity(items.len());
    for raw in items {
        let mut parts = raw.splitn(3, ':');
        let recipient_id = parts.next().unwrap_or_default().trim();
        let key_id = parts.next().unwrap_or_default().trim();
        let public_key_hex = parts.next().unwrap_or_default().trim();

        if recipient_id.is_empty() || key_id.is_empty() || public_key_hex.is_empty() {
            anyhow::bail!(
                "invalid --recipient-key '{}'; expected recipient-id:key-id:hex-x25519-public-key",
                raw
            );
        }

        let public_key_bytes = hex::decode(public_key_hex).map_err(|err| {
            anyhow::anyhow!(
                "invalid --recipient-key '{}'; public key must be 32-byte hex ({err})",
                raw
            )
        })?;
        let public_key: [u8; 32] = public_key_bytes.as_slice().try_into().map_err(|_| {
            anyhow::anyhow!(
                "invalid --recipient-key '{}'; public key must be exactly 32 bytes",
                raw
            )
        })?;

        out.push(RecipientPublicKey {
            recipient_id: recipient_id.to_string(),
            key_id: key_id.to_string(),
            public_key,
        });
    }

    Ok(out)
}

pub fn run(args: ShipArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let config = load_or_default_config(&root)?;

    // Load intent
    let intent_obj_id = store
        .get_ref(&format!("intents/{}", args.intent))?
        .ok_or_else(|| anyhow::anyhow!("intent not found: {}", args.intent))?;
    let intent_obj = store.load_object(&intent_obj_id)?;
    let mut intent = match intent_obj {
        Object::Intent(i) => i,
        _ => anyhow::bail!("not an intent"),
    };

    // Load revision
    let rev_id = store
        .get_ref(&args.revision_ref)?
        .ok_or_else(|| anyhow::anyhow!("ref not found: {}", args.revision_ref))?;
    let revision_obj = store.load_object(&rev_id)?;
    let revision = match revision_obj {
        Object::Revision(r) => r,
        _ => anyhow::bail!("ref is not a revision: {}", args.revision_ref),
    };

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;

    let mut evidence = parse_evidence(&args.evidence)?;
    enrich_evidence_for_revision(&mut evidence, &rev_id, now_ms, &args);
    let acceptance_results = if args.run_acceptance {
        let results = run_acceptance_for_ship(
            &root,
            &intent,
            &rev_id,
            args.acceptance_timeout_ms,
            args.acceptance_keep_going,
        )?;
        evidence.extend(results.iter().map(acceptance_result_evidence));
        if let Some(failed) = results
            .iter()
            .find(|result| result.outcome != AcceptanceOutcome::Passed)
        {
            anyhow::bail!(
                "acceptance test '{}' {} for intent {}; capsule was not written",
                failed.name,
                failed.outcome.status(),
                intent.id
            );
        }
        results
    } else {
        Vec::new()
    };

    let registered_agent = ensure_registered_signing_agent(&store, &args.agent)?;
    let keypair = keypair_for_agent(&args.agent, &registered_agent)?;
    let mut signing_agents = vec![registered_agent.clone()];

    let public = CapsulePublic {
        agent_id: registered_agent.agent_id.clone(),
        agent_version: registered_agent.agent_version.clone(),
        toolchain_digest: None,
        env_fingerprint: None,
        evidence,
    };
    let evidence_count = public.evidence.len();

    let recipients = parse_recipient_public_keys(&args.recipient_keys)?;
    let mut capsule = if let Some(private_file) = args.private_file.as_deref() {
        if recipients.is_empty() {
            anyhow::bail!("--private-file requires at least one --recipient-key");
        }
        let private_data = std::fs::read(private_file)?;
        build_capsule_for_recipients(&rev_id, public, &private_data, &recipients, &keypair)?
    } else {
        if !recipients.is_empty() {
            anyhow::bail!("--recipient-key requires --private-file");
        }
        build_capsule(&rev_id, public, None, None, &keypair)?
    };
    capsule.key_id = Some(registered_agent.public_key.clone());
    for signer in &args.co_sign {
        let cosigner = ensure_registered_signing_agent(&store, signer)?;
        let cosigner_key = keypair_for_agent(signer, &cosigner)?;
        append_capsule_signature(&mut capsule, &cosigner_key)?;
        signing_agents.push(cosigner);
    }

    if config.policy.fail_closed_ship {
        enforce_ship_policies(
            &store,
            &intent,
            &revision,
            &rev_id,
            &capsule,
            &signing_agents,
        )?;
    }

    let capsule_id = store.store_object(&Object::Capsule(capsule))?;

    // Add capsule mapping refs
    store.set_ref(&format!("capsules/{}", rev_id.to_hex()), &capsule_id)?;
    store.set_ref(
        &format!("capsules/by-revision/{}", rev_id.to_hex()),
        &capsule_id,
    )?;
    // Backward-compatible short reverse key.
    store.set_ref(
        &format!("capsules/by-revision/{}", &rev_id.to_hex()[..16]),
        &capsule_id,
    )?;

    let mut finalized_change: Option<ChangeId> = revision.change_id;
    if finalized_change.is_none() && intent.change_ids.len() == 1 {
        if let Ok(single_change) = ChangeId::from_string(&intent.change_ids[0]) {
            finalized_change = Some(single_change);
        }
    }

    if finalized_change.is_none() && intent.change_ids.is_empty() {
        anyhow::bail!(
            "intent {} has no linked changes; create one with `claw change create --intent {}`",
            intent.id,
            intent.id
        );
    }

    if let Some(change_id) = finalized_change {
        let change_ref = format!("changes/{change_id}");
        let Some(change_obj_id) = store.get_ref(&change_ref)? else {
            anyhow::bail!("change not found: {change_id}");
        };
        let change_obj = store.load_object(&change_obj_id)?;
        let Object::Change(mut change) = change_obj else {
            anyhow::bail!("ref {} is not a change", change_ref);
        };
        if change.intent_id != intent.id {
            anyhow::bail!(
                "change {} does not belong to intent {}",
                change.id,
                intent.id
            );
        }
        change.head_revision = Some(rev_id);
        change.status = ChangeStatus::Integrated;
        change.updated_at_ms = now_ms;
        let new_change_id = store.store_object(&Object::Change(change.clone()))?;
        store.set_ref(&change_ref, &new_change_id)?;

        let change_id_string = change.id.to_string();
        if !intent.change_ids.iter().any(|id| id == &change_id_string) {
            intent.change_ids.push(change_id_string);
        }
    }

    // Update intent status coherently with associated changes.
    let mut updated_intent = intent;
    let mut intent_changed = false;

    let all_integrated = if updated_intent.change_ids.is_empty() {
        false
    } else {
        let mut all_integrated = true;
        for change_id in &updated_intent.change_ids {
            let Some(change_obj_id) = store.get_ref(&format!("changes/{change_id}"))? else {
                all_integrated = false;
                break;
            };
            let change_obj = store.load_object(&change_obj_id)?;
            let Object::Change(change) = change_obj else {
                all_integrated = false;
                break;
            };
            if change.intent_id != updated_intent.id {
                all_integrated = false;
                break;
            }
            if change.status != ChangeStatus::Integrated {
                all_integrated = false;
                break;
            }
        }
        all_integrated
    };

    if all_integrated {
        if updated_intent.status != IntentStatus::Done
            && updated_intent.status != IntentStatus::Superseded
        {
            updated_intent.status = IntentStatus::Done;
            intent_changed = true;
        }
    } else if updated_intent.status == IntentStatus::Done {
        updated_intent.status = IntentStatus::Open;
        intent_changed = true;
    }

    if intent_changed {
        updated_intent.updated_at_ms = now_ms;
    }

    let new_intent_id = store.store_object(&Object::Intent(updated_intent.clone()))?;
    store.set_ref(&format!("intents/{}", updated_intent.id), &new_intent_id)?;

    if args.json {
        let co_signers = args.co_sign.clone();
        let revision_ref = args.revision_ref.clone();
        let agent_id = registered_agent.agent_id.clone();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "ship",
                "intent_id": updated_intent.id.to_string(),
                "intent_status": updated_intent.status,
                "change_id": finalized_change.map(|id| id.to_string()),
                "revision_ref": revision_ref,
                "revision_id": rev_id.to_string(),
                "capsule_id": capsule_id.to_string(),
                "agent_id": agent_id,
                "co_signers": co_signers,
                "signature_count": signing_agents.len(),
                "evidence_count": evidence_count,
                "acceptance": {
                    "run": args.run_acceptance,
                    "passed": if args.run_acceptance { Some(true) } else { None },
                    "count": acceptance_results.len(),
                    "timeout_ms": args.acceptance_timeout_ms,
                    "keep_going": args.acceptance_keep_going,
                    "evidence_names": acceptance_results.iter().map(|result| result.name.clone()).collect::<Vec<_>>(),
                },
                "private_fields_encrypted": args.private_file.is_some(),
                "recipient_count": recipients.len(),
            }))?
        );
    } else {
        println!("Shipped intent: {}", updated_intent.id);
        println!("  Capsule: {capsule_id}");
        println!("  Revision: {rev_id}");
    }

    Ok(())
}

fn enforce_ship_policies(
    store: &ClawStore,
    intent: &Intent,
    revision: &Revision,
    rev_id: &ObjectId,
    capsule: &Capsule,
    signing_agents: &[AgentRegistration],
) -> anyhow::Result<()> {
    let policies = load_policies_for_intent(store, intent)?;
    if policies.is_empty() {
        return Ok(());
    }

    let (signer_agent_ids, signer_key_ids) = policy_signers(signing_agents);
    let context = PolicyContext {
        revision_id: Some(*rev_id),
        signer_agent_ids,
        signer_key_ids,
        touched_paths: collect_touched_paths(store, revision)?,
        trust_score: derive_capsule_trust_score(capsule),
        now_ms: Some(current_time_ms()),
    };

    for policy in policies {
        evaluate_policy(&policy, revision, capsule, &context).map_err(|err| {
            anyhow::anyhow!(
                "policy '{}' blocked shipping revision {} for intent {}: {}. Add the required evidence/reviewer signatures (for example with --evidence/--co-sign) or set policy.fail_closed_ship = false in .claw/config.toml",
                policy.policy_id,
                rev_id.to_hex(),
                intent.id,
                err
            )
        })?;
    }

    Ok(())
}

fn enrich_evidence_for_revision(
    evidence: &mut [Evidence],
    rev_id: &ObjectId,
    now_ms: u64,
    args: &ShipArgs,
) {
    for item in evidence {
        item.revision_id.get_or_insert(*rev_id);
        item.started_at_ms.get_or_insert(now_ms);
        item.ended_at_ms.get_or_insert(now_ms);
        item.exit_code
            .get_or_insert(if item.status.eq_ignore_ascii_case("pass") {
                0
            } else {
                1
            });
        if item.command.is_none() {
            item.command = args.evidence_command.clone();
        }
        if item.runner_identity.is_none() {
            item.runner_identity = args.runner_identity.clone();
        }
        if item.environment_digest.is_none() {
            item.environment_digest = args.environment_digest.clone();
        }
        if item.log_digest.is_none() {
            item.log_digest = args.log_digest.clone();
        }
        if item.artifact_digest.is_none() {
            item.artifact_digest = args.artifact_digest.clone();
        }
        if item.expires_at_ms.is_none() {
            item.expires_at_ms = args
                .evidence_expires_in_ms
                .map(|ttl| now_ms.saturating_add(ttl));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcceptanceOutcome {
    Passed,
    Failed,
    TimedOut,
}

impl AcceptanceOutcome {
    fn status(self) -> &'static str {
        match self {
            Self::Passed => "pass",
            Self::Failed => "fail",
            Self::TimedOut => "timeout",
        }
    }
}

#[derive(Debug)]
struct AcceptanceResult {
    name: String,
    command: String,
    outcome: AcceptanceOutcome,
    exit_code: Option<i32>,
    duration_ms: u64,
    started_at_ms: u64,
    ended_at_ms: u64,
    revision_id: ObjectId,
}

fn run_acceptance_for_ship(
    root: &std::path::Path,
    intent: &Intent,
    revision_id: &ObjectId,
    timeout_ms: u64,
    keep_going: bool,
) -> anyhow::Result<Vec<AcceptanceResult>> {
    let mut results = Vec::new();
    for (index, command) in intent.acceptance_tests.iter().enumerate() {
        let result = run_acceptance_command_for_ship(
            root,
            format!("acceptance/{}", index + 1),
            command,
            timeout_ms,
            revision_id,
        )?;
        let failed = result.outcome != AcceptanceOutcome::Passed;
        results.push(result);
        if failed && !keep_going {
            break;
        }
    }
    Ok(results)
}

fn run_acceptance_command_for_ship(
    root: &std::path::Path,
    name: String,
    command: &str,
    timeout_ms: u64,
    revision_id: &ObjectId,
) -> anyhow::Result<AcceptanceResult> {
    let started_at_ms = current_time_ms();
    let start = Instant::now();
    let mut child = shell_command(command)
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| {
            anyhow::anyhow!("failed to run acceptance command '{}': {}", command, err)
        })?;

    let timeout = Duration::from_millis(timeout_ms);
    let (outcome, exit_code) = loop {
        if let Some(status) = child.try_wait()? {
            let outcome = if status.success() {
                AcceptanceOutcome::Passed
            } else {
                AcceptanceOutcome::Failed
            };
            break (outcome, status.code());
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            break (AcceptanceOutcome::TimedOut, None);
        }

        std::thread::sleep(Duration::from_millis(25));
    };
    let ended_at_ms = current_time_ms();
    let duration_ms = start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;

    Ok(AcceptanceResult {
        name,
        command: command.to_string(),
        outcome,
        exit_code,
        duration_ms,
        started_at_ms,
        ended_at_ms,
        revision_id: *revision_id,
    })
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }

    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

fn acceptance_result_evidence(result: &AcceptanceResult) -> Evidence {
    Evidence {
        name: result.name.clone(),
        status: result.outcome.status().to_string(),
        duration_ms: result.duration_ms,
        artifact_refs: Vec::new(),
        summary: Some(format!("{} {}", result.name, result.outcome.status())),
        revision_id: Some(result.revision_id),
        command: Some(result.command.clone()),
        exit_code: result.exit_code,
        started_at_ms: Some(result.started_at_ms),
        ended_at_ms: Some(result.ended_at_ms),
        environment_digest: None,
        runner_identity: None,
        log_digest: None,
        artifact_digest: None,
        expires_at_ms: None,
        trust_domain: Some("acceptance".to_string()),
        signature: None,
    }
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn load_policies_for_intent(store: &ClawStore, intent: &Intent) -> anyhow::Result<Vec<Policy>> {
    if intent.policy_refs.is_empty() {
        return Ok(vec![]);
    }

    let mut policies = Vec::new();
    let mut seen = HashSet::new();
    for policy_ref in &intent.policy_refs {
        let policy = load_policy_ref(store, policy_ref)?;
        if seen.insert(policy.policy_id.clone()) {
            policies.push(policy);
        }
    }

    Ok(policies)
}

fn load_policy_ref(store: &ClawStore, policy_ref: &str) -> anyhow::Result<Policy> {
    let ref_name = if store.get_ref(policy_ref)?.is_some() {
        policy_ref.to_string()
    } else {
        format!("policies/{policy_ref}")
    };

    let policy_obj_id = store
        .get_ref(&ref_name)?
        .ok_or_else(|| anyhow::anyhow!("policy ref not found: {}", ref_name))?;
    let policy_obj = store.load_object(&policy_obj_id)?;
    match policy_obj {
        Object::Policy(policy) => Ok(policy),
        _ => anyhow::bail!("ref does not point to a policy object: {}", ref_name),
    }
}

fn policy_signers(signing_agents: &[AgentRegistration]) -> (Vec<String>, Vec<String>) {
    let mut signer_agent_ids = Vec::new();
    let mut signer_key_ids = Vec::new();
    let mut seen_agents = HashSet::new();
    let mut seen_keys = HashSet::new();

    for agent in signing_agents {
        let agent_id = agent.agent_id.trim();
        if !agent_id.is_empty() {
            let normalized = agent_id.to_ascii_lowercase();
            if seen_agents.insert(normalized) {
                signer_agent_ids.push(agent_id.to_string());
            }
        }

        let key_id = agent.public_key.trim().to_ascii_lowercase();
        if !key_id.is_empty() && seen_keys.insert(key_id.clone()) {
            signer_key_ids.push(key_id);
        }
    }

    signer_agent_ids.sort();
    signer_key_ids.sort();
    (signer_agent_ids, signer_key_ids)
}

fn derive_capsule_trust_score(capsule: &Capsule) -> Option<f32> {
    let total = capsule.public_fields.evidence.len();
    if total == 0 {
        return None;
    }

    let passed = capsule
        .public_fields
        .evidence
        .iter()
        .filter(|e| e.status.eq_ignore_ascii_case("pass"))
        .count();

    Some(passed as f32 / total as f32)
}

fn collect_touched_paths(store: &ClawStore, revision: &Revision) -> anyhow::Result<Vec<String>> {
    let mut touched_paths = Vec::new();
    let mut seen = HashSet::new();

    for patch_id in &revision.patches {
        let patch_obj = store.load_object(patch_id)?;
        let patch = match patch_obj {
            Object::Patch(patch) => patch,
            _ => anyhow::bail!(
                "revision patch reference does not point to a patch object: {}",
                patch_id.to_hex()
            ),
        };

        let path = patch.target_path.trim();
        if path.is_empty() {
            continue;
        }
        if seen.insert(path.to_string()) {
            touched_paths.push(path.to_string());
        }
    }

    Ok(touched_paths)
}

#[cfg(test)]
mod tests {
    use super::{
        acceptance_result_evidence, collect_touched_paths, derive_capsule_trust_score,
        parse_evidence, parse_recipient_public_keys, policy_signers, AcceptanceOutcome,
        AcceptanceResult,
    };
    use crate::commands::agent::AgentRegistration;
    use claw_core::object::{Object, TypeTag};
    use claw_core::types::{Capsule, CapsulePublic, Evidence, Patch, Revision};
    use claw_store::ClawStore;

    fn test_store() -> (tempfile::TempDir, ClawStore) {
        let temp = tempfile::tempdir().unwrap();
        let store = ClawStore::init(temp.path()).unwrap();
        (temp, store)
    }

    #[test]
    fn parses_evidence_with_equals_and_optional_duration() {
        let parsed =
            parse_evidence(&["test=pass:1200".to_string(), "lint=pass".to_string()]).unwrap();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "test");
        assert_eq!(parsed[0].status, "pass");
        assert_eq!(parsed[0].duration_ms, 1200);
        assert_eq!(parsed[1].name, "lint");
        assert_eq!(parsed[1].status, "pass");
        assert_eq!(parsed[1].duration_ms, 0);
    }

    #[test]
    fn rejects_invalid_evidence_format() {
        assert!(parse_evidence(&["broken".to_string()]).is_err());
        assert!(parse_evidence(&["=pass".to_string()]).is_err());
        assert!(parse_evidence(&["test=".to_string()]).is_err());
        assert!(parse_evidence(&["test=pass:notnum".to_string()]).is_err());
    }

    #[test]
    fn acceptance_result_maps_to_revision_bound_evidence() {
        let revision_id = claw_core::hash::content_hash(TypeTag::Revision, b"acceptance");
        let evidence = acceptance_result_evidence(&AcceptanceResult {
            name: "acceptance/1".to_string(),
            command: "cargo test -p app".to_string(),
            outcome: AcceptanceOutcome::Passed,
            exit_code: Some(0),
            duration_ms: 123,
            started_at_ms: 1_000,
            ended_at_ms: 1_123,
            revision_id,
        });

        assert_eq!(evidence.name, "acceptance/1");
        assert_eq!(evidence.status, "pass");
        assert_eq!(evidence.duration_ms, 123);
        assert_eq!(evidence.revision_id, Some(revision_id));
        assert_eq!(evidence.command.as_deref(), Some("cargo test -p app"));
        assert_eq!(evidence.exit_code, Some(0));
        assert_eq!(evidence.started_at_ms, Some(1_000));
        assert_eq!(evidence.ended_at_ms, Some(1_123));
        assert_eq!(evidence.trust_domain.as_deref(), Some("acceptance"));
    }

    #[test]
    fn parses_recipient_public_key_envelope_input() {
        let parsed =
            parse_recipient_public_keys(&[format!("security:security-key:{}", "07".repeat(32))])
                .unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].recipient_id, "security");
        assert_eq!(parsed[0].key_id, "security-key");
        assert_eq!(parsed[0].public_key, [7u8; 32]);
        assert!(parse_recipient_public_keys(&["security:missing-key".to_string()]).is_err());
        assert!(
            parse_recipient_public_keys(&[format!("security:key:{}", "07".repeat(31))]).is_err()
        );
    }

    #[test]
    fn derives_capsule_trust_score_from_pass_ratio() {
        let revision_id = claw_core::hash::content_hash(TypeTag::Revision, b"ship");
        let capsule = Capsule {
            revision_id,
            public_fields: CapsulePublic {
                agent_id: "agent".to_string(),
                agent_version: None,
                toolchain_digest: None,
                env_fingerprint: None,
                evidence: vec![
                    Evidence {
                        name: "ci/test".to_string(),
                        status: "PASS".to_string(),
                        duration_ms: 0,
                        artifact_refs: vec![],
                        summary: None,
                        revision_id: None,
                        command: None,
                        exit_code: None,
                        started_at_ms: None,
                        ended_at_ms: None,
                        environment_digest: None,
                        runner_identity: None,
                        log_digest: None,
                        artifact_digest: None,
                        expires_at_ms: None,
                        trust_domain: None,
                        signature: None,
                    },
                    Evidence {
                        name: "lint".to_string(),
                        status: "fail".to_string(),
                        duration_ms: 0,
                        artifact_refs: vec![],
                        summary: None,
                        revision_id: None,
                        command: None,
                        exit_code: None,
                        started_at_ms: None,
                        ended_at_ms: None,
                        environment_digest: None,
                        runner_identity: None,
                        log_digest: None,
                        artifact_digest: None,
                        expires_at_ms: None,
                        trust_domain: None,
                        signature: None,
                    },
                ],
            },
            encrypted_private: None,
            encryption: String::new(),
            key_id: None,
            recipients: vec![],
            signatures: vec![],
        };

        assert_eq!(derive_capsule_trust_score(&capsule), Some(0.5));
    }

    #[test]
    fn policy_signers_deduplicate_agent_ids_and_keys() {
        let signers = vec![
            AgentRegistration {
                schema_version: 2,
                agent_id: "agent-a".to_string(),
                agent_version: None,
                public_key: "ABCDEF".to_string(),
                private_key: None,
                revoked_at_ms: None,
                revocation_reason: None,
                quarantined_at_ms: None,
                quarantine_reason: None,
                created_at_ms: 1,
                updated_at_ms: 1,
            },
            AgentRegistration {
                schema_version: 2,
                agent_id: "AGENT-A".to_string(),
                agent_version: None,
                public_key: "abcdef".to_string(),
                private_key: None,
                revoked_at_ms: None,
                revocation_reason: None,
                quarantined_at_ms: None,
                quarantine_reason: None,
                created_at_ms: 1,
                updated_at_ms: 1,
            },
            AgentRegistration {
                schema_version: 2,
                agent_id: "agent-b".to_string(),
                agent_version: None,
                public_key: "1234".to_string(),
                private_key: None,
                revoked_at_ms: None,
                revocation_reason: None,
                quarantined_at_ms: None,
                quarantine_reason: None,
                created_at_ms: 1,
                updated_at_ms: 1,
            },
        ];

        let (agent_ids, key_ids) = policy_signers(&signers);
        assert_eq!(
            agent_ids,
            vec!["agent-a".to_string(), "agent-b".to_string()]
        );
        assert_eq!(key_ids, vec!["1234".to_string(), "abcdef".to_string()]);
    }

    #[test]
    fn collects_touched_paths_from_revision_patches() {
        let (_tmp, store) = test_store();
        let patch_a_id = store
            .store_object(&Object::Patch(Patch {
                target_path: "src/main.rs".to_string(),
                codec_id: "text".to_string(),
                base_object: None,
                result_object: None,
                ops: vec![],
                codec_payload: None,
            }))
            .unwrap();
        let patch_b_id = store
            .store_object(&Object::Patch(Patch {
                target_path: " src/main.rs ".to_string(),
                codec_id: "text".to_string(),
                base_object: None,
                result_object: None,
                ops: vec![],
                codec_payload: None,
            }))
            .unwrap();
        let patch_c_id = store
            .store_object(&Object::Patch(Patch {
                target_path: "".to_string(),
                codec_id: "text".to_string(),
                base_object: None,
                result_object: None,
                ops: vec![],
                codec_payload: None,
            }))
            .unwrap();

        let revision = Revision {
            change_id: None,
            parents: vec![],
            patches: vec![patch_a_id, patch_b_id, patch_c_id],
            snapshot_base: None,
            tree: None,
            capsule_id: None,
            author: "test".to_string(),
            created_at_ms: 1,
            summary: "ship".to_string(),
            policy_evidence: vec![],
        };

        let touched = collect_touched_paths(&store, &revision).unwrap();
        assert_eq!(touched, vec!["src/main.rs".to_string()]);
    }
}
