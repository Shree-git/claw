use clap::Args;

use std::collections::{HashMap, HashSet, VecDeque};

use claw_core::id::ChangeId;
use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::{Capsule, Conflict, PatchOp, Policy, Revision};
use claw_crypto::capsule::verify_capsule;
use claw_merge::emit::merge;
use claw_patch::CodecRegistry;
use claw_policy::{evaluator::evaluate_policy, PolicyContext};
use claw_store::{ClawStore, HeadState};

use super::agent::AgentRegistration;
use crate::config::find_repo_root;
use crate::conflict_writer;
use crate::merge_state::{self, ConflictEntry, ConflictRegion, MergeInfo, MergeState};
use crate::worktree;

#[derive(Args)]
pub struct IntegrateArgs {
    /// Output a machine-readable JSON receipt
    #[arg(long)]
    json: bool,
    /// Left ref (default: HEAD's branch)
    #[arg(long)]
    left: Option<String>,
    /// Right ref to integrate
    #[arg(long)]
    right: String,
    /// Author name
    #[arg(short, long, default_value = "anonymous")]
    author: String,
    /// Merge message
    #[arg(short, long, default_value = "Integrate changes")]
    message: String,
    /// Preview policy checks and merge result without updating refs or worktree
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Default)]
struct AgentRegistry {
    by_public_key: HashMap<String, String>,
    by_agent_id: HashMap<String, String>,
}

#[derive(Debug, Default)]
struct ProvenanceContext {
    signer_agent_ids: Vec<String>,
    signer_key_ids: Vec<String>,
    trust_score: Option<f32>,
}

pub fn run(args: IntegrateArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let registry = CodecRegistry::default();

    // Resolve left ref: default to HEAD's branch
    let left_ref = match args.left {
        Some(r) => r,
        None => {
            let head = store.read_head()?;
            match head {
                HeadState::Symbolic { ref_name } => ref_name,
                HeadState::Detached { .. } => {
                    anyhow::bail!("cannot integrate in detached HEAD state; use --left to specify")
                }
            }
        }
    };

    let left_id = store
        .get_ref(&left_ref)?
        .ok_or_else(|| anyhow::anyhow!("ref not found: {}", left_ref))?;
    let right_id = resolve_revision_ref_or_id(&store, &args.right)?;

    enforce_integration_policies(&store, &left_id, &right_id)?;

    let result = merge(
        &store,
        &registry,
        &left_id,
        &right_id,
        &args.author,
        &args.message,
    )?;

    if result.conflicts.is_empty() {
        if args.dry_run {
            if args.json {
                print_integrate_json(IntegrateJson {
                    dry_run: true,
                    clean: true,
                    left_ref: &left_ref,
                    right_ref: &args.right,
                    left_id,
                    right_id,
                    base_id: result.ancestor,
                    result_revision: None,
                    result_tree: result.revision.tree,
                    ref_updated: false,
                    worktree_updated: false,
                    merge_state_written: false,
                    conflicts: vec![],
                })?;
            } else {
                println!("Dry run: integration can be applied cleanly.");
                println!("  Left ref: {left_ref} ({left_id})");
                println!("  Right: {} ({right_id})", args.right);
                if let Some(tree_id) = result.revision.tree {
                    println!("  Result tree: {tree_id}");
                }
                println!("  Ref update skipped.");
                println!("  Worktree update skipped.");
            }
            return Ok(());
        }

        // Clean merge: store revision, materialize tree, advance ref
        let result_tree = result.revision.tree;
        let rev_id = store.store_object(&Object::Revision(result.revision))?;
        store.update_ref_cas(
            &left_ref,
            Some(&left_id),
            &rev_id,
            &args.author,
            &args.message,
        )?;

        // Materialize merged tree
        if let Some(tree_id) = store.load_object(&rev_id)?.as_revision_tree() {
            worktree::materialize_tree(&store, &tree_id, &root)?;
        }

        if args.json {
            print_integrate_json(IntegrateJson {
                dry_run: false,
                clean: true,
                left_ref: &left_ref,
                right_ref: &args.right,
                left_id,
                right_id,
                base_id: result.ancestor,
                result_revision: Some(rev_id),
                result_tree,
                ref_updated: true,
                worktree_updated: true,
                merge_state_written: false,
                conflicts: vec![],
            })?;
        } else {
            println!("Integrated successfully: {rev_id}");
        }
    } else {
        if args.dry_run {
            if args.json {
                let conflicts = conflict_receipts(&store, &result.conflicts)?;
                print_integrate_json(IntegrateJson {
                    dry_run: true,
                    clean: false,
                    left_ref: &left_ref,
                    right_ref: &args.right,
                    left_id,
                    right_id,
                    base_id: result.ancestor,
                    result_revision: None,
                    result_tree: result.revision.tree,
                    ref_updated: false,
                    worktree_updated: false,
                    merge_state_written: false,
                    conflicts,
                })?;
            } else {
                println!(
                    "Dry run: merge would have {} conflict(s).",
                    result.conflicts.len()
                );
                for c in &result.conflicts {
                    print_conflict_explanation(&store, c)?;
                }
                println!("  Conflict artifacts skipped.");
                println!("  Merge state write skipped.");
            }
            return Ok(());
        }

        // Conflicted merge: write conflict artifacts, MERGE_STATE.toml, do NOT advance ref
        let mut conflict_entries = Vec::new();

        for conflict in &result.conflicts {
            let base_content =
                load_file_from_revision(&store, &result.ancestor, &conflict.file_path);
            let left_content = load_file_from_revision(&store, &left_id, &conflict.file_path);
            let right_content = load_file_from_revision(&store, &right_id, &conflict.file_path);

            let conflict_id = claw_core::id::ConflictId::new().to_string();

            match conflict.codec_id.as_str() {
                "json/tree" => {
                    conflict_writer::write_json_conflict(
                        &root,
                        &conflict.file_path,
                        &base_content,
                        &left_content,
                        &right_content,
                    )?;
                }
                "binary" => {
                    conflict_writer::write_binary_conflict(
                        &root,
                        &conflict.file_path,
                        &base_content,
                        &left_content,
                        &right_content,
                    )?;
                }
                _ => {
                    conflict_writer::write_text_conflict(
                        &root,
                        &conflict.file_path,
                        &base_content,
                        &left_content,
                        &right_content,
                        &left_ref,
                        &args.right,
                    )?;
                }
            }

            let explanation = explain_conflict(&store, conflict)?;
            conflict_entries.push(ConflictEntry {
                file_path: conflict.file_path.clone(),
                conflict_id,
                codec_id: conflict.codec_id.clone(),
                reason: Some(explanation.reason),
                regions: explanation.regions,
            });
        }

        // Write MERGE_STATE.toml
        let merge_state = MergeState {
            merge: MergeInfo {
                left_ref: left_ref.clone(),
                right_ref: args.right.clone(),
                left_revision: left_id.to_hex(),
                right_revision: right_id.to_hex(),
                base_revision: result.ancestor.to_hex(),
            },
            conflicts: conflict_entries,
        };
        merge_state::write_to(&store.layout().claw_dir(), &merge_state)?;

        // Also materialize non-conflicting changes from the merged tree
        // Use the left tree as the base for the working copy
        let left_obj = store.load_object(&left_id)?;
        if let Object::Revision(ref rev) = left_obj {
            if let Some(ref tree_id) = rev.tree {
                worktree::materialize_tree(&store, tree_id, &root)?;
            }
        }

        if args.json {
            let conflicts = conflict_receipts(&store, &result.conflicts)?;
            print_integrate_json(IntegrateJson {
                dry_run: false,
                clean: false,
                left_ref: &left_ref,
                right_ref: &args.right,
                left_id,
                right_id,
                base_id: result.ancestor,
                result_revision: None,
                result_tree: result.revision.tree,
                ref_updated: false,
                worktree_updated: true,
                merge_state_written: true,
                conflicts,
            })?;
        } else {
            println!(
                "Merge has {} conflict(s). Resolve them and run `claw snapshot` to complete.",
                result.conflicts.len()
            );
            for c in &result.conflicts {
                print_conflict_explanation(&store, c)?;
            }
        }
    }

    Ok(())
}

struct IntegrateJson<'a> {
    dry_run: bool,
    clean: bool,
    left_ref: &'a str,
    right_ref: &'a str,
    left_id: ObjectId,
    right_id: ObjectId,
    base_id: ObjectId,
    result_revision: Option<ObjectId>,
    result_tree: Option<ObjectId>,
    ref_updated: bool,
    worktree_updated: bool,
    merge_state_written: bool,
    conflicts: Vec<serde_json::Value>,
}

fn print_integrate_json(receipt: IntegrateJson<'_>) -> anyhow::Result<()> {
    let value = serde_json::json!({
        "schema_version": 1,
        "action": "integrate",
        "dry_run": receipt.dry_run,
        "clean": receipt.clean,
        "left_ref": receipt.left_ref,
        "right_ref": receipt.right_ref,
        "left_revision": receipt.left_id.to_string(),
        "right_revision": receipt.right_id.to_string(),
        "base_revision": receipt.base_id.to_string(),
        "result_revision": receipt.result_revision.map(|id| id.to_string()),
        "result_tree": receipt.result_tree.map(|id| id.to_string()),
        "ref_updated": receipt.ref_updated,
        "worktree_updated": receipt.worktree_updated,
        "merge_state_written": receipt.merge_state_written,
        "conflict_count": receipt.conflicts.len(),
        "conflicts": receipt.conflicts,
    });
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn conflict_receipts(
    store: &ClawStore,
    conflicts: &[Conflict],
) -> anyhow::Result<Vec<serde_json::Value>> {
    conflicts
        .iter()
        .map(|conflict| {
            let explanation = explain_conflict(store, conflict)?;
            Ok(serde_json::json!({
                "path": &conflict.file_path,
                "codec": &conflict.codec_id,
                "reason": explanation.reason,
                "regions": explanation.regions.iter().map(|region| serde_json::json!({
                    "side": &region.side,
                    "patch_id": &region.patch_id,
                    "op_type": &region.op_type,
                    "address": &region.address,
                    "old_bytes": region.old_bytes,
                    "new_bytes": region.new_bytes,
                })).collect::<Vec<_>>(),
            }))
        })
        .collect()
}

struct ConflictExplanation {
    reason: String,
    regions: Vec<ConflictRegion>,
}

fn print_conflict_explanation(store: &ClawStore, conflict: &Conflict) -> anyhow::Result<()> {
    let explanation = explain_conflict(store, conflict)?;
    println!("  CONFLICT: {} ({})", conflict.file_path, conflict.codec_id);
    println!("    reason: {}", explanation.reason);
    if !explanation.regions.is_empty() {
        println!("    collided regions:");
        for region in &explanation.regions {
            println!(
                "      {} {} at {} ({})",
                region.side, region.op_type, region.address, region.patch_id
            );
        }
    }
    Ok(())
}

fn explain_conflict(store: &ClawStore, conflict: &Conflict) -> anyhow::Result<ConflictExplanation> {
    let left_regions = collect_conflict_regions(store, "left", &conflict.left_patch_ids)?;
    let right_regions = collect_conflict_regions(store, "right", &conflict.right_patch_ids)?;
    let mut regions = left_regions.clone();
    regions.extend(right_regions.clone());

    let left_addresses = left_regions
        .iter()
        .map(|region| region.address.as_str())
        .collect::<Vec<_>>();
    let right_addresses = right_regions
        .iter()
        .map(|region| region.address.as_str())
        .collect::<Vec<_>>();
    let shared = left_addresses
        .iter()
        .copied()
        .filter(|address| right_addresses.contains(address))
        .collect::<Vec<_>>();

    let region = if !shared.is_empty() {
        shared
            .iter()
            .map(|address| describe_region(&conflict.codec_id, address))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        let left = left_addresses
            .first()
            .map(|address| describe_region(&conflict.codec_id, address))
            .unwrap_or_else(|| "the left patch stream".to_string());
        let right = right_addresses
            .first()
            .map(|address| describe_region(&conflict.codec_id, address))
            .unwrap_or_else(|| "the right patch stream".to_string());
        format!("{left} and {right}")
    };

    let left_ops = summarize_side_ops(&left_regions);
    let right_ops = summarize_side_ops(&right_regions);
    let reason = format!(
        "left {left_ops} and right {right_ops} both touched {region}; the {} codec could not commute those operations or produce a conflict-free three-way merge",
        conflict.codec_id
    );

    Ok(ConflictExplanation { reason, regions })
}

fn collect_conflict_regions(
    store: &ClawStore,
    side: &str,
    patch_ids: &[ObjectId],
) -> anyhow::Result<Vec<ConflictRegion>> {
    let mut regions = Vec::new();
    for patch_id in patch_ids {
        let Object::Patch(patch) = store.load_object(patch_id)? else {
            continue;
        };
        for op in &patch.ops {
            regions.push(region_from_op(side, patch_id, op));
        }
    }
    Ok(regions)
}

fn region_from_op(side: &str, patch_id: &ObjectId, op: &PatchOp) -> ConflictRegion {
    ConflictRegion {
        side: side.to_string(),
        patch_id: patch_id.to_string(),
        op_type: op.op_type.clone(),
        address: op.address.clone(),
        old_bytes: op.old_data.as_ref().map(Vec::len),
        new_bytes: op.new_data.as_ref().map(Vec::len),
    }
}

fn summarize_side_ops(regions: &[ConflictRegion]) -> String {
    if regions.is_empty() {
        return "made no inspectable operation".to_string();
    }
    let mut parts = regions
        .iter()
        .map(|region| format!("{} at {}", region.op_type, region.address))
        .collect::<Vec<_>>();
    parts.sort();
    parts.dedup();
    parts.join(", ")
}

fn describe_region(codec_id: &str, address: &str) -> String {
    match codec_id {
        "text/line" => address
            .strip_prefix('L')
            .and_then(|line| line.parse::<usize>().ok())
            .map(|line| format!("line {}", line + 1))
            .unwrap_or_else(|| format!("text region {address}")),
        "json/tree" => {
            if address == "/" {
                "JSON root".to_string()
            } else {
                format!("JSON path {address}")
            }
        }
        "binary" => "the binary payload".to_string(),
        _ => format!("semantic address {address}"),
    }
}

fn resolve_revision_ref_or_id(store: &ClawStore, value: &str) -> anyhow::Result<ObjectId> {
    if let Some(id) = store.get_ref(value)? {
        return Ok(id);
    }

    if let Ok(id) = ObjectId::from_hex(value) {
        return ensure_revision_id(store, id, value);
    }

    if let Ok(id) = ObjectId::from_display(value) {
        return ensure_revision_id(store, id, value);
    }

    anyhow::bail!("ref not found: {value}")
}

fn ensure_revision_id(store: &ClawStore, id: ObjectId, source: &str) -> anyhow::Result<ObjectId> {
    match store.load_object(&id)? {
        Object::Revision(_) => Ok(id),
        _ => anyhow::bail!("not a revision: {source}"),
    }
}

fn enforce_integration_policies(
    store: &ClawStore,
    left_id: &ObjectId,
    right_id: &ObjectId,
) -> anyhow::Result<()> {
    let applicable = collect_applicable_revisions(store, left_id, right_id)?;
    let agent_registry = load_agent_registry(store)?;

    for rev_id in applicable {
        let rev = load_revision(store, &rev_id)?;
        let policies = load_policies_for_revision(store, &rev)?;
        if policies.is_empty() {
            continue;
        }

        let capsule = load_capsule_for_revision(store, &rev_id, &rev)?.ok_or_else(|| {
            anyhow::anyhow!(
                "policy-gated revision {} has no capsule evidence",
                rev_id.to_hex()
            )
        })?;
        let provenance = verify_capsule_provenance(&capsule, &rev_id, &agent_registry)?;
        let touched_paths = collect_touched_paths(store, &rev)?;
        let context = PolicyContext {
            revision_id: Some(rev_id),
            signer_agent_ids: provenance.signer_agent_ids,
            signer_key_ids: provenance.signer_key_ids,
            touched_paths,
            trust_score: provenance.trust_score,
            now_ms: Some(current_time_ms()),
        };

        for policy in policies {
            evaluate_policy(&policy, &rev, &capsule, &context).map_err(|err| {
                anyhow::anyhow!(
                    "policy '{}' failed for revision {}: {}",
                    policy.policy_id,
                    rev_id.to_hex(),
                    err
                )
            })?;
        }
    }

    Ok(())
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn verify_capsule_provenance(
    capsule: &Capsule,
    rev_id: &ObjectId,
    registry: &AgentRegistry,
) -> anyhow::Result<ProvenanceContext> {
    if capsule.revision_id != *rev_id {
        anyhow::bail!(
            "capsule revision mismatch: capsule={}, revision={}",
            capsule.revision_id.to_hex(),
            rev_id.to_hex()
        );
    }

    if capsule.signatures.is_empty() {
        anyhow::bail!("capsule has no signatures");
    }

    let mut valid_signers = HashSet::new();
    let mut malformed_signers = 0usize;

    for sig in &capsule.signatures {
        let normalized_signer = sig.signer_id.trim().to_ascii_lowercase();
        let signer_bytes = match hex::decode(&normalized_signer) {
            Ok(bytes) => bytes,
            Err(_) => {
                malformed_signers += 1;
                continue;
            }
        };
        let signer_key: [u8; 32] = match signer_bytes.as_slice().try_into() {
            Ok(key) => key,
            Err(_) => {
                malformed_signers += 1;
                continue;
            }
        };

        let mut candidate = capsule.clone();
        candidate.signatures = vec![sig.clone()];
        if matches!(verify_capsule(&candidate, &signer_key), Ok(true)) {
            valid_signers.insert(normalized_signer);
        }
    }

    if valid_signers.is_empty() {
        if malformed_signers > 0 {
            anyhow::bail!(
                "capsule signatures failed verification ({} malformed signer id(s))",
                malformed_signers
            );
        }
        anyhow::bail!("capsule signatures failed verification");
    }

    let agent_id = capsule.public_fields.agent_id.trim();
    if agent_id.is_empty() {
        anyhow::bail!("capsule public agent_id is empty");
    }
    let expected_key = registry
        .by_agent_id
        .get(agent_id)
        .ok_or_else(|| anyhow::anyhow!("capsule agent '{}' is not registered", agent_id))?;
    if !valid_signers.contains(expected_key) {
        anyhow::bail!(
            "capsule signer does not match registered key for agent '{}'",
            agent_id
        );
    }

    let mut signer_key_ids: Vec<String> = valid_signers.into_iter().collect();
    signer_key_ids.sort();

    let mut signer_agent_ids: Vec<String> = signer_key_ids
        .iter()
        .filter_map(|signer| registry.by_public_key.get(signer).cloned())
        .collect();
    signer_agent_ids.sort();
    signer_agent_ids.dedup();

    Ok(ProvenanceContext {
        signer_agent_ids,
        signer_key_ids,
        trust_score: derive_capsule_trust_score(capsule),
    })
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

fn load_agent_registry(store: &ClawStore) -> anyhow::Result<AgentRegistry> {
    let mut registry = AgentRegistry::default();
    for (_name, obj_id) in store.list_refs("agents")? {
        let obj = store.load_object(&obj_id)?;
        let Object::Blob(blob) = obj else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<AgentRegistration>(&blob.data) else {
            continue;
        };
        if record.is_revoked() || record.is_quarantined() {
            continue;
        }
        let normalized_key = match normalize_public_key_hex(&record.public_key) {
            Some(key) => key,
            None => continue,
        };
        registry
            .by_agent_id
            .insert(record.agent_id.clone(), normalized_key.clone());
        registry
            .by_public_key
            .insert(normalized_key, record.agent_id);
    }
    Ok(registry)
}

fn normalize_public_key_hex(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    let bytes = hex::decode(&normalized).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    Some(normalized)
}

fn collect_touched_paths(store: &ClawStore, rev: &Revision) -> anyhow::Result<Vec<String>> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    for patch_id in &rev.patches {
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
            paths.push(path.to_string());
        }
    }

    Ok(paths)
}

fn collect_applicable_revisions(
    store: &ClawStore,
    left_id: &ObjectId,
    right_id: &ObjectId,
) -> anyhow::Result<Vec<ObjectId>> {
    let left_reachable = collect_reachable_revisions(store, left_id)?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([*right_id]);

    while let Some(id) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        if left_reachable.contains(&id) {
            continue;
        }

        let rev = load_revision(store, &id)?;
        out.push(id);
        for parent in rev.parents {
            queue.push_back(parent);
        }
    }

    Ok(out)
}

fn collect_reachable_revisions(
    store: &ClawStore,
    start: &ObjectId,
) -> anyhow::Result<HashSet<ObjectId>> {
    let mut reachable = HashSet::new();
    let mut queue = VecDeque::from([*start]);

    while let Some(id) = queue.pop_front() {
        if !reachable.insert(id) {
            continue;
        }
        let rev = load_revision(store, &id)?;
        for parent in rev.parents {
            queue.push_back(parent);
        }
    }

    Ok(reachable)
}

fn load_revision(store: &ClawStore, id: &ObjectId) -> anyhow::Result<Revision> {
    let obj = store.load_object(id)?;
    match obj {
        Object::Revision(rev) => Ok(rev),
        _ => anyhow::bail!("object is not a revision: {}", id.to_hex()),
    }
}

fn load_policies_for_revision(store: &ClawStore, rev: &Revision) -> anyhow::Result<Vec<Policy>> {
    let change_id = match rev.change_id {
        Some(id) => id,
        None => return Ok(vec![]),
    };

    let intent = load_intent_for_change(store, &change_id)?;
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

fn load_intent_for_change(
    store: &ClawStore,
    change_id: &ChangeId,
) -> anyhow::Result<claw_core::types::Intent> {
    let change_obj_id = store
        .get_ref(&format!("changes/{change_id}"))?
        .ok_or_else(|| anyhow::anyhow!("change not found: {}", change_id))?;
    let change_obj = store.load_object(&change_obj_id)?;
    let change = match change_obj {
        Object::Change(c) => c,
        _ => anyhow::bail!(
            "change ref does not point to a change object: {}",
            change_id
        ),
    };

    let intent_obj_id = store
        .get_ref(&format!("intents/{}", change.intent_id))?
        .ok_or_else(|| anyhow::anyhow!("intent not found for change: {}", change_id))?;
    let intent_obj = store.load_object(&intent_obj_id)?;
    match intent_obj {
        Object::Intent(intent) => Ok(intent),
        _ => anyhow::bail!(
            "intent ref does not point to an intent object: {}",
            change.intent_id
        ),
    }
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

fn load_capsule_for_revision(
    store: &ClawStore,
    rev_id: &ObjectId,
    rev: &Revision,
) -> anyhow::Result<Option<Capsule>> {
    let capsule_id = if let Some(id) = rev.capsule_id {
        Some(id)
    } else {
        let full = rev_id.to_hex();
        store
            .get_ref(&format!("capsules/by-revision/{full}"))?
            .or_else(|| {
                let prefix = &full[..16];
                store
                    .get_ref(&format!("capsules/by-revision/{prefix}"))
                    .ok()
                    .flatten()
            })
    };

    let Some(capsule_id) = capsule_id else {
        return Ok(None);
    };
    let capsule_obj = store.load_object(&capsule_id)?;
    match capsule_obj {
        Object::Capsule(capsule) => Ok(Some(capsule)),
        _ => anyhow::bail!(
            "capsule mapping points to non-capsule object for revision {}",
            rev_id.to_hex()
        ),
    }
}

fn load_file_from_revision(store: &ClawStore, rev_id: &ObjectId, path: &str) -> Vec<u8> {
    let obj = match store.load_object(rev_id) {
        Ok(o) => o,
        Err(_) => return vec![],
    };
    let tree_id = match obj {
        Object::Revision(ref rev) => match rev.tree {
            Some(t) => t,
            None => return vec![],
        },
        _ => return vec![],
    };
    find_blob_in_tree(store, &tree_id, path).unwrap_or_default()
}

fn find_blob_in_tree(store: &ClawStore, tree_id: &ObjectId, path: &str) -> Option<Vec<u8>> {
    let parts: Vec<&str> = path.split('/').collect();
    find_blob_recursive(store, tree_id, &parts)
}

fn find_blob_recursive(store: &ClawStore, tree_id: &ObjectId, parts: &[&str]) -> Option<Vec<u8>> {
    if parts.is_empty() {
        return None;
    }
    let obj = store.load_object(tree_id).ok()?;
    let tree = match obj {
        Object::Tree(t) => t,
        _ => return None,
    };
    for entry in &tree.entries {
        if entry.name == parts[0] {
            if parts.len() == 1 {
                let blob_obj = store.load_object(&entry.object_id).ok()?;
                if let Object::Blob(b) = blob_obj {
                    return Some(b.data);
                }
                return None;
            } else {
                return find_blob_recursive(store, &entry.object_id, &parts[1..]);
            }
        }
    }
    None
}

// Helper trait for Object
trait ObjectExt {
    fn as_revision_tree(&self) -> Option<ObjectId>;
}

impl ObjectExt for Object {
    fn as_revision_tree(&self) -> Option<ObjectId> {
        match self {
            Object::Revision(rev) => rev.tree,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        collect_applicable_revisions, enforce_integration_policies, load_agent_registry,
        resolve_revision_ref_or_id, verify_capsule_provenance, AgentRegistry, IntegrateArgs,
    };
    use crate::commands::agent::AgentRegistration;
    use clap::Parser;
    use claw_core::id::{ChangeId, IntentId};
    use claw_core::object::Object;
    use claw_core::types::Blob;
    use claw_core::types::{
        Change, ChangeStatus, EvidencePolicy, Intent, IntentStatus, Policy, Revision, Visibility,
    };
    use claw_crypto::capsule::build_capsule;
    use claw_crypto::keypair::KeyPair;
    use claw_store::ClawStore;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: IntegrateArgs,
    }

    #[test]
    fn parses_dry_run_flag() {
        let cli = TestCli::parse_from(["claw", "--right", "heads/feature", "--dry-run"]);

        assert_eq!(cli.args.right, "heads/feature");
        assert!(cli.args.dry_run);
    }

    fn test_store() -> (tempfile::TempDir, ClawStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ClawStore::init(tmp.path()).unwrap();
        (tmp, store)
    }

    fn register_agent(store: &ClawStore, name: &str, keypair: &KeyPair) {
        let now = 1;
        let record = AgentRegistration {
            schema_version: 2,
            agent_id: name.to_string(),
            agent_version: Some("test".to_string()),
            public_key: hex::encode(keypair.public_key_bytes()),
            private_key: None,
            revoked_at_ms: None,
            revocation_reason: None,
            quarantined_at_ms: None,
            quarantine_reason: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        let blob = Object::Blob(Blob {
            data: serde_json::to_vec(&record).unwrap(),
            media_type: Some("application/json".to_string()),
        });
        let blob_id = store.store_object(&blob).unwrap();
        store.set_ref(&format!("agents/{name}"), &blob_id).unwrap();
    }

    #[test]
    fn agent_registry_skips_revoked_registrations() {
        let (_tmp, store) = test_store();
        let keypair = KeyPair::generate();
        let record = AgentRegistration {
            schema_version: 2,
            agent_id: "agent".to_string(),
            agent_version: Some("test".to_string()),
            public_key: hex::encode(keypair.public_key_bytes()),
            private_key: None,
            revoked_at_ms: Some(2),
            revocation_reason: Some("compromised".to_string()),
            quarantined_at_ms: None,
            quarantine_reason: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        let blob = Object::Blob(Blob {
            data: serde_json::to_vec(&record).unwrap(),
            media_type: Some("application/json".to_string()),
        });
        let blob_id = store.store_object(&blob).unwrap();
        store.set_ref("agents/agent", &blob_id).unwrap();

        let registry = load_agent_registry(&store).unwrap();
        assert!(registry.by_agent_id.is_empty());
        assert!(registry.by_public_key.is_empty());
    }

    #[test]
    fn agent_registry_skips_quarantined_registrations() {
        let (_tmp, store) = test_store();
        let keypair = KeyPair::generate();
        let record = AgentRegistration {
            schema_version: 2,
            agent_id: "agent".to_string(),
            agent_version: Some("test".to_string()),
            public_key: hex::encode(keypair.public_key_bytes()),
            private_key: None,
            revoked_at_ms: None,
            revocation_reason: None,
            quarantined_at_ms: Some(2),
            quarantine_reason: Some("runner drift".to_string()),
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        let blob = Object::Blob(Blob {
            data: serde_json::to_vec(&record).unwrap(),
            media_type: Some("application/json".to_string()),
        });
        let blob_id = store.store_object(&blob).unwrap();
        store.set_ref("agents/agent", &blob_id).unwrap();

        let registry = load_agent_registry(&store).unwrap();
        assert!(registry.by_agent_id.is_empty());
        assert!(registry.by_public_key.is_empty());
    }

    #[test]
    fn collect_applicable_revisions_excludes_left_side() {
        let (_tmp, store) = test_store();

        let base = store
            .store_object(&Object::Revision(Revision {
                change_id: None,
                parents: vec![],
                patches: vec![],
                snapshot_base: None,
                tree: None,
                capsule_id: None,
                author: "test".to_string(),
                created_at_ms: 1,
                summary: "base".to_string(),
                policy_evidence: vec![],
            }))
            .unwrap();

        let right_only = store
            .store_object(&Object::Revision(Revision {
                change_id: None,
                parents: vec![base],
                patches: vec![],
                snapshot_base: None,
                tree: None,
                capsule_id: None,
                author: "test".to_string(),
                created_at_ms: 2,
                summary: "right".to_string(),
                policy_evidence: vec![],
            }))
            .unwrap();

        let applicable = collect_applicable_revisions(&store, &base, &right_only).unwrap();
        assert_eq!(applicable, vec![right_only]);
    }

    #[test]
    fn policy_check_blocks_integration_when_evidence_missing() {
        let (_tmp, store) = test_store();

        let intent_id = IntentId::new();
        let change_id = ChangeId::new();

        let intent_obj = Object::Intent(Intent {
            id: intent_id,
            title: "intent".to_string(),
            goal: "goal".to_string(),
            constraints: vec![],
            acceptance_tests: vec![],
            links: vec![],
            policy_refs: vec!["ci-required".to_string()],
            agents: vec![],
            change_ids: vec![],
            depends_on: vec![],
            supersedes: vec![],
            status: IntentStatus::Open,
            created_at_ms: 1,
            updated_at_ms: 1,
        });
        let intent_obj_id = store.store_object(&intent_obj).unwrap();
        store
            .set_ref(&format!("intents/{intent_id}"), &intent_obj_id)
            .unwrap();

        let change_obj = Object::Change(Change {
            id: change_id,
            intent_id,
            head_revision: None,
            workstream_id: None,
            status: ChangeStatus::Open,
            created_at_ms: 1,
            updated_at_ms: 1,
        });
        let change_obj_id = store.store_object(&change_obj).unwrap();
        store
            .set_ref(&format!("changes/{change_id}"), &change_obj_id)
            .unwrap();

        let policy_obj = Object::Policy(Policy {
            policy_id: "ci-required".to_string(),
            required_checks: vec!["ci/test".to_string()],
            required_reviewers: vec![],
            sensitive_paths: vec![],
            quarantine_lane: false,
            min_trust_score: None,
            visibility: Visibility::Public,
            authorized_recipients: vec![],
            revoked_recipients: vec![],
            evidence_policy: EvidencePolicy::default(),
        });
        let policy_obj_id = store.store_object(&policy_obj).unwrap();
        store
            .set_ref("policies/ci-required", &policy_obj_id)
            .unwrap();

        let left = store
            .store_object(&Object::Revision(Revision {
                change_id: None,
                parents: vec![],
                patches: vec![],
                snapshot_base: None,
                tree: None,
                capsule_id: None,
                author: "test".to_string(),
                created_at_ms: 2,
                summary: "left".to_string(),
                policy_evidence: vec![],
            }))
            .unwrap();

        let right = store
            .store_object(&Object::Revision(Revision {
                change_id: Some(change_id),
                parents: vec![left],
                patches: vec![],
                snapshot_base: None,
                tree: None,
                capsule_id: None,
                author: "test".to_string(),
                created_at_ms: 3,
                summary: "right".to_string(),
                policy_evidence: vec![],
            }))
            .unwrap();

        let keypair = KeyPair::generate();
        register_agent(&store, "agent", &keypair);
        let capsule_obj = Object::Capsule(
            build_capsule(
                &right,
                claw_core::types::CapsulePublic {
                    agent_id: "agent".to_string(),
                    agent_version: None,
                    toolchain_digest: None,
                    env_fingerprint: None,
                    evidence: vec![],
                },
                None,
                None,
                &keypair,
            )
            .unwrap(),
        );
        let capsule_id = store.store_object(&capsule_obj).unwrap();
        store
            .set_ref(
                &format!("capsules/by-revision/{}", &right.to_hex()[..16]),
                &capsule_id,
            )
            .unwrap();

        let err = enforce_integration_policies(&store, &left, &right).unwrap_err();
        assert!(
            err.to_string().contains("missing required check"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn provenance_check_rejects_revision_mismatch() {
        let keypair = KeyPair::generate();
        let revision_a = claw_core::hash::content_hash(claw_core::object::TypeTag::Revision, b"a");
        let revision_b = claw_core::hash::content_hash(claw_core::object::TypeTag::Revision, b"b");

        let capsule = build_capsule(
            &revision_a,
            claw_core::types::CapsulePublic {
                agent_id: "agent".to_string(),
                agent_version: None,
                toolchain_digest: None,
                env_fingerprint: None,
                evidence: vec![],
            },
            None,
            None,
            &keypair,
        )
        .unwrap();

        let err = verify_capsule_provenance(&capsule, &revision_b, &AgentRegistry::default())
            .unwrap_err();
        assert!(err.to_string().contains("capsule revision mismatch"));
    }

    #[test]
    fn provenance_check_rejects_unregistered_agent() {
        let keypair = KeyPair::generate();
        let revision = claw_core::hash::content_hash(claw_core::object::TypeTag::Revision, b"rev");

        let capsule = build_capsule(
            &revision,
            claw_core::types::CapsulePublic {
                agent_id: "unknown".to_string(),
                agent_version: None,
                toolchain_digest: None,
                env_fingerprint: None,
                evidence: vec![],
            },
            None,
            None,
            &keypair,
        )
        .unwrap();

        let err =
            verify_capsule_provenance(&capsule, &revision, &AgentRegistry::default()).unwrap_err();
        assert!(err.to_string().contains("is not registered"));
    }

    #[test]
    fn resolve_revision_ref_or_id_accepts_display_id() {
        let (_tmp, store) = test_store();
        let rev_id = store
            .store_object(&Object::Revision(Revision {
                change_id: None,
                parents: vec![],
                patches: vec![],
                snapshot_base: None,
                tree: None,
                capsule_id: None,
                author: "test".to_string(),
                created_at_ms: 1,
                summary: "rev".to_string(),
                policy_evidence: vec![],
            }))
            .unwrap();

        let resolved = resolve_revision_ref_or_id(&store, &rev_id.to_string()).unwrap();
        assert_eq!(resolved, rev_id);
    }

    #[test]
    fn resolve_revision_ref_or_id_rejects_non_revision_object() {
        let (_tmp, store) = test_store();
        let blob_id = store
            .store_object(&Object::Blob(Blob {
                data: b"blob".to_vec(),
                media_type: None,
            }))
            .unwrap();

        let err = resolve_revision_ref_or_id(&store, &blob_id.to_string()).unwrap_err();
        assert!(err.to_string().contains("not a revision"));
    }
}
