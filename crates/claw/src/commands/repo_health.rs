use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use claw_core::object::Object;
use claw_store::ClawStore;
use claw_store::{reflog, refs};

use super::agent::AgentRegistration;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DeepHealthReport {
    pub object_count: usize,
    pub ref_count: usize,
    pub issue_count: usize,
    pub repairable_count: usize,
    pub summary: DeepHealthSummary,
    pub issues: Vec<HealthIssue>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct DeepHealthSummary {
    pub error_count: usize,
    pub warning_count: usize,
    pub corruption_count: usize,
    pub invalid_ref_namespace_count: usize,
    pub dangling_ref_count: usize,
    pub missing_capsule_count: usize,
    pub capsule_index_drift_count: usize,
    pub policy_drift_count: usize,
    pub weak_key_count: usize,
    pub repairable_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HealthIssue {
    pub code: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair: Option<RepairPlan>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RepairPlan {
    pub kind: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

impl DeepHealthReport {
    pub(crate) fn errors(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.severity == "error")
            .count()
    }
}

pub(crate) fn scan(store: &ClawStore) -> DeepHealthReport {
    let refs = store.list_refs("").unwrap_or_default();
    let object_ids = store.list_object_ids().unwrap_or_default();
    let mut issues = Vec::new();

    for namespace_issue in inspect_ref_namespace(store.root()) {
        issues.push(HealthIssue {
            code: "invalid_ref_namespace".to_string(),
            severity: "error".to_string(),
            message: format!("invalid ref namespace entry: {namespace_issue}"),
            object: None,
            ref_name: None,
            repair: None,
        });
    }

    for (ref_name, target) in &refs {
        if !store.has_object(target) {
            let rollback_target = rollback_candidate(store, ref_name, target);
            issues.push(HealthIssue {
                code: "missing_ref_target".to_string(),
                severity: "error".to_string(),
                message: format!(
                    "ref {ref_name} points to missing object {}",
                    target.to_hex()
                ),
                object: Some(target.to_hex()),
                ref_name: Some(ref_name.clone()),
                repair: Some(RepairPlan {
                    kind: "ref_rollback".to_string(),
                    description: rollback_target
                        .as_ref()
                        .map(|candidate| {
                            format!(
                                "roll {ref_name} back to last existing reflog target {candidate}"
                            )
                        })
                        .unwrap_or_else(|| {
                            "manual rollback required: inspect reflog and choose a valid old target"
                                .to_string()
                        }),
                    ref_name: Some(ref_name.clone()),
                    target: rollback_target,
                }),
            });
            continue;
        }

        if ref_name.starts_with("agents/") {
            add_agent_key_issues(store, &mut issues, ref_name, target);
        }
    }

    add_reflog_recovery_issues(store, &refs, &mut issues);

    let live_intent_ids = refs
        .iter()
        .filter(|(ref_name, _)| ref_name.starts_with("intents/"))
        .map(|(_, target)| target.to_hex())
        .collect::<BTreeSet<_>>();

    for id in &object_ids {
        let object = match store.load_object(id) {
            Ok(object) => object,
            Err(err) => {
                issues.push(HealthIssue {
                    code: "corrupt_object".to_string(),
                    severity: "error".to_string(),
                    message: format!("object {} failed to decode: {err}", id.to_hex()),
                    object: Some(id.to_hex()),
                    ref_name: None,
                    repair: Some(RepairPlan {
                        kind: "object_restore".to_string(),
                        description:
                            "restore this object from backup or a trusted remote before using dependent refs"
                                .to_string(),
                        ref_name: None,
                        target: Some(id.to_hex()),
                    }),
                });
                continue;
            }
        };

        for dep in object.dependencies() {
            if !store.has_object(&dep) {
                issues.push(HealthIssue {
                    code: "missing_dependency".to_string(),
                    severity: "error".to_string(),
                    message: format!(
                        "{} object {} depends on missing object {}",
                        object.type_tag().name(),
                        id.to_hex(),
                        dep.to_hex()
                    ),
                    object: Some(id.to_hex()),
                    ref_name: None,
                    repair: Some(RepairPlan {
                        kind: "object_restore".to_string(),
                        description: "restore the missing dependency from backup or remote"
                            .to_string(),
                        ref_name: None,
                        target: Some(dep.to_hex()),
                    }),
                });
            }
        }

        match object {
            Object::Capsule(capsule) => {
                if !store.has_object(&capsule.revision_id) {
                    issues.push(HealthIssue {
                        code: "capsule_missing_revision".to_string(),
                        severity: "error".to_string(),
                        message: format!(
                            "capsule {} claims missing revision {}",
                            id.to_hex(),
                            capsule.revision_id.to_hex()
                        ),
                        object: Some(id.to_hex()),
                        ref_name: None,
                        repair: None,
                    });
                }
                add_capsule_index_issues(store, &mut issues, id, &capsule.revision_id);
            }
            Object::Revision(revision) => {
                if let Some(capsule_id) = revision.capsule_id {
                    if !store.has_object(&capsule_id) {
                        issues.push(HealthIssue {
                            code: "revision_missing_capsule".to_string(),
                            severity: "error".to_string(),
                            message: format!(
                                "revision {} references missing capsule {}",
                                id.to_hex(),
                                capsule_id.to_hex()
                            ),
                            object: Some(id.to_hex()),
                            ref_name: None,
                            repair: None,
                        });
                    }
                }
            }
            Object::Intent(intent) => {
                if !live_intent_ids.contains(&id.to_hex()) {
                    continue;
                }
                for policy_ref in &intent.policy_refs {
                    let ref_name = if policy_ref.starts_with("policies/") {
                        policy_ref.clone()
                    } else {
                        format!("policies/{policy_ref}")
                    };
                    if store.get_ref(&ref_name).ok().flatten().is_none() {
                        issues.push(HealthIssue {
                            code: "missing_intent_policy".to_string(),
                            severity: "warning".to_string(),
                            message: format!(
                                "intent {} references missing policy {}",
                                intent.id, ref_name
                            ),
                            object: Some(id.to_hex()),
                            ref_name: Some(ref_name.clone()),
                            repair: Some(RepairPlan {
                                kind: "policy_audit_regeneration".to_string(),
                                description:
                                    "remove stale policy refs from the intent audit metadata"
                                        .to_string(),
                                ref_name: Some(format!("intents/{}", intent.id)),
                                target: Some(ref_name),
                            }),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    let repairable_count = issues
        .iter()
        .filter(|issue| issue.repair.as_ref().is_some_and(is_safe_automatic_repair))
        .count();
    let summary = summarize_issues(&issues, repairable_count);
    DeepHealthReport {
        object_count: object_ids.len(),
        ref_count: refs.len(),
        issue_count: issues.len(),
        repairable_count,
        summary,
        issues,
    }
}

pub(crate) fn inspect_ref_namespace(root: &Path) -> Vec<String> {
    let refs_root = root.join(".claw").join("refs");
    let mut issues = Vec::new();
    collect_ref_namespace_issues(&refs_root, &refs_root, &mut issues);
    issues.sort();
    issues.dedup();
    issues
}

fn collect_ref_namespace_issues(refs_root: &Path, dir: &Path, issues: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());

    let mut folded_siblings = BTreeMap::<String, String>::new();
    for entry in &entries {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            issues.push(format!(
                "non-utf8 path under {}",
                display_relative(refs_root, &path)
            ));
            continue;
        };
        let folded = portable_case_key(&name);
        let rel = display_relative(refs_root, &path);
        if let Some(existing) = folded_siblings.insert(folded, rel.clone()) {
            if existing != rel {
                issues.push(format!(
                    "case-insensitive collision between {existing} and {rel}"
                ));
            }
        }
    }

    for entry in entries {
        let path = entry.path();
        let rel = display_relative(refs_root, &path);
        let Ok(file_type) = entry.file_type() else {
            issues.push(format!("unreadable ref namespace entry {rel}"));
            continue;
        };

        if file_type.is_dir() {
            if refs::validate_ref_name(&rel).is_err() {
                issues.push(format!("invalid ref path component {rel}"));
            }
            collect_ref_namespace_issues(refs_root, &path, issues);
        } else if file_type.is_file() {
            if refs::validate_ref_name(&rel).is_err() {
                issues.push(format!("invalid ref name {rel}"));
            }
        } else {
            issues.push(format!("unsupported ref namespace entry {rel}"));
        }
    }
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn portable_case_key(value: &str) -> String {
    value.chars().flat_map(|ch| ch.to_lowercase()).collect()
}

fn summarize_issues(issues: &[HealthIssue], repairable_count: usize) -> DeepHealthSummary {
    let mut summary = DeepHealthSummary {
        repairable_count,
        ..DeepHealthSummary::default()
    };

    for issue in issues {
        match issue.severity.as_str() {
            "error" => summary.error_count += 1,
            "warning" => summary.warning_count += 1,
            _ => {}
        }
        match issue.code.as_str() {
            "corrupt_object" | "missing_dependency" => summary.corruption_count += 1,
            "invalid_ref_namespace" => summary.invalid_ref_namespace_count += 1,
            "missing_ref_target" | "deleted_ref_with_reflog" => summary.dangling_ref_count += 1,
            "revision_missing_capsule" | "capsule_missing_revision" => {
                summary.missing_capsule_count += 1
            }
            "capsule_index_drift" | "capsule_index_unreadable" => {
                summary.capsule_index_drift_count += 1
            }
            "missing_intent_policy" => summary.policy_drift_count += 1,
            "agent_private_key_material"
            | "agent_weak_public_key"
            | "agent_malformed_public_key" => summary.weak_key_count += 1,
            _ => {}
        }
    }

    summary
}

fn add_agent_key_issues(
    store: &ClawStore,
    issues: &mut Vec<HealthIssue>,
    ref_name: &str,
    target: &claw_core::id::ObjectId,
) {
    let Ok(Object::Blob(blob)) = store.load_object(target) else {
        return;
    };
    let Ok(record) = serde_json::from_slice::<AgentRegistration>(&blob.data) else {
        return;
    };

    if record.private_key.is_some() {
        issues.push(HealthIssue {
            code: "agent_private_key_material".to_string(),
            severity: "error".to_string(),
            message: format!(
                "agent {} stores private key material in repository metadata",
                record.agent_id
            ),
            object: Some(target.to_hex()),
            ref_name: Some(ref_name.to_string()),
            repair: None,
        });
    }

    match hex::decode(&record.public_key) {
        Ok(bytes) if bytes.len() == 32 && bytes.iter().any(|byte| *byte != 0) => {}
        Ok(bytes) if bytes.len() == 32 => issues.push(HealthIssue {
            code: "agent_weak_public_key".to_string(),
            severity: "error".to_string(),
            message: format!("agent {} has an all-zero public key", record.agent_id),
            object: Some(target.to_hex()),
            ref_name: Some(ref_name.to_string()),
            repair: None,
        }),
        Ok(bytes) => issues.push(HealthIssue {
            code: "agent_malformed_public_key".to_string(),
            severity: "error".to_string(),
            message: format!(
                "agent {} public key decodes to {} byte(s), expected 32",
                record.agent_id,
                bytes.len()
            ),
            object: Some(target.to_hex()),
            ref_name: Some(ref_name.to_string()),
            repair: None,
        }),
        Err(err) => issues.push(HealthIssue {
            code: "agent_malformed_public_key".to_string(),
            severity: "error".to_string(),
            message: format!(
                "agent {} public key is not valid hex: {err}",
                record.agent_id
            ),
            object: Some(target.to_hex()),
            ref_name: Some(ref_name.to_string()),
            repair: None,
        }),
    }
}

fn rollback_candidate(
    store: &ClawStore,
    ref_name: &str,
    missing_target: &claw_core::id::ObjectId,
) -> Option<String> {
    let entries = reflog::read_reflog(store.layout(), ref_name).ok()?;
    existing_reflog_target(store, &entries, Some(missing_target))
}

fn add_reflog_recovery_issues(
    store: &ClawStore,
    refs: &[(String, claw_core::id::ObjectId)],
    issues: &mut Vec<HealthIssue>,
) {
    let existing_refs = refs
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<BTreeSet<_>>();
    for ref_name in list_reflog_refs(store) {
        if existing_refs.contains(ref_name.as_str()) {
            continue;
        }
        let Ok(entries) = reflog::read_reflog(store.layout(), &ref_name) else {
            continue;
        };
        let Some(target) = existing_reflog_target(store, &entries, None) else {
            continue;
        };
        issues.push(HealthIssue {
            code: "deleted_ref_with_reflog".to_string(),
            severity: "warning".to_string(),
            message: format!("ref {ref_name} is missing but its reflog can recover it at {target}"),
            object: Some(target.clone()),
            ref_name: Some(ref_name.clone()),
            repair: Some(RepairPlan {
                kind: "ref_recovery".to_string(),
                description: format!(
                    "restore {ref_name} to newest existing reflog target {target}"
                ),
                ref_name: Some(ref_name),
                target: Some(target),
            }),
        });
    }
}

fn list_reflog_refs(store: &ClawStore) -> Vec<String> {
    let root = store.layout().reflogs_dir();
    let mut refs = Vec::new();
    collect_reflog_refs(&root, &root, &mut refs);
    refs.sort();
    refs
}

fn collect_reflog_refs(root: &std::path::Path, dir: &std::path::Path, refs: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            collect_reflog_refs(root, &path, refs);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let Some(ref_name) = relative.to_str() else {
            continue;
        };
        refs.push(ref_name.to_string());
    }
}

fn existing_reflog_target(
    store: &ClawStore,
    entries: &[reflog::RefLogLine],
    excluded_target: Option<&claw_core::id::ObjectId>,
) -> Option<String> {
    let zero = claw_core::id::ObjectId::from_bytes([0; 32]);
    entries
        .iter()
        .rev()
        .flat_map(|entry| [entry.new, entry.old])
        .find(|candidate| {
            excluded_target.is_none_or(|excluded| *candidate != *excluded)
                && *candidate != zero
                && store.has_object(candidate)
        })
        .map(|candidate| candidate.to_hex())
}

pub(crate) fn is_safe_automatic_repair(repair: &RepairPlan) -> bool {
    match repair.kind.as_str() {
        "capsule_reindex" => repair.ref_name.is_some() && repair.target.is_some(),
        "ref_rollback" => repair.ref_name.is_some() && repair.target.is_some(),
        "ref_recovery" => repair.ref_name.is_some() && repair.target.is_some(),
        "policy_audit_regeneration" => repair.ref_name.is_some() && repair.target.is_some(),
        _ => false,
    }
}

fn add_capsule_index_issues(
    store: &ClawStore,
    issues: &mut Vec<HealthIssue>,
    capsule_id: &claw_core::id::ObjectId,
    revision_id: &claw_core::id::ObjectId,
) {
    for ref_name in [
        format!("capsules/{}", revision_id.to_hex()),
        format!("capsules/by-revision/{}", revision_id.to_hex()),
        format!("capsules/by-revision/{}", &revision_id.to_hex()[..16]),
    ] {
        match store.get_ref(&ref_name) {
            Ok(Some(target)) if target == *capsule_id => {}
            Ok(existing) => {
                let message = match existing {
                    Some(target) => format!(
                        "capsule index {ref_name} points to {}, expected {}",
                        target.to_hex(),
                        capsule_id.to_hex()
                    ),
                    None => format!(
                        "capsule index {ref_name} is missing for capsule {}",
                        capsule_id.to_hex()
                    ),
                };
                issues.push(HealthIssue {
                    code: "capsule_index_drift".to_string(),
                    severity: "warning".to_string(),
                    message,
                    object: Some(capsule_id.to_hex()),
                    ref_name: Some(ref_name.clone()),
                    repair: Some(RepairPlan {
                        kind: "capsule_reindex".to_string(),
                        description: format!("set {ref_name} to {}", capsule_id.to_hex()),
                        ref_name: Some(ref_name),
                        target: Some(capsule_id.to_hex()),
                    }),
                });
            }
            Err(err) => issues.push(HealthIssue {
                code: "capsule_index_unreadable".to_string(),
                severity: "error".to_string(),
                message: format!("cannot read capsule index {ref_name}: {err}"),
                object: Some(capsule_id.to_hex()),
                ref_name: Some(ref_name),
                repair: None,
            }),
        }
    }
}

pub(crate) fn apply_safe_repairs(store: &ClawStore) -> anyhow::Result<Vec<RepairPlan>> {
    let report = scan(store);
    let mut applied = Vec::new();
    for issue in report.issues {
        let Some(repair) = issue.repair else {
            continue;
        };
        match repair.kind.as_str() {
            "capsule_reindex" => {
                apply_ref_target(store, &repair)?;
                applied.push(repair);
            }
            "ref_rollback" => {
                apply_ref_target(store, &repair)?;
                applied.push(repair);
            }
            "ref_recovery" => {
                apply_ref_target(store, &repair)?;
                applied.push(repair);
            }
            _ => {}
        }
    }
    applied.extend(apply_policy_audit_regeneration(store)?);
    Ok(applied)
}

fn apply_ref_target(store: &ClawStore, repair: &RepairPlan) -> anyhow::Result<()> {
    let Some(ref_name) = repair.ref_name.as_deref() else {
        return Ok(());
    };
    let Some(target) = repair.target.as_deref() else {
        return Ok(());
    };
    let target = claw_core::id::ObjectId::from_hex(target)?;
    store.set_ref(ref_name, &target)?;
    Ok(())
}

fn apply_policy_audit_regeneration(store: &ClawStore) -> anyhow::Result<Vec<RepairPlan>> {
    let mut applied = Vec::new();
    let mut intent_repairs = BTreeMap::<String, Vec<String>>::new();
    for issue in scan(store).issues {
        let Some(repair) = issue.repair else {
            continue;
        };
        if repair.kind != "policy_audit_regeneration" || !is_safe_automatic_repair(&repair) {
            continue;
        }
        let Some(intent_ref) = repair.ref_name.clone() else {
            continue;
        };
        let Some(policy_ref) = repair.target.clone() else {
            continue;
        };
        intent_repairs
            .entry(intent_ref)
            .or_default()
            .push(policy_ref);
    }

    for (intent_ref, missing_policy_refs) in intent_repairs {
        let Some(intent_obj_id) = store.get_ref(&intent_ref)? else {
            continue;
        };
        let Object::Intent(mut intent) = store.load_object(&intent_obj_id)? else {
            continue;
        };
        let before = intent.policy_refs.len();
        intent.policy_refs.retain(|policy_ref| {
            let normalized = if policy_ref.starts_with("policies/") {
                policy_ref.clone()
            } else {
                format!("policies/{policy_ref}")
            };
            !missing_policy_refs
                .iter()
                .any(|missing| missing == &normalized)
        });
        if intent.policy_refs.len() == before {
            continue;
        }
        intent.updated_at_ms = current_time_ms();
        let new_id = store.store_object(&Object::Intent(intent.clone()))?;
        store.set_ref(&intent_ref, &new_id)?;
        applied.push(RepairPlan {
            kind: "policy_audit_regeneration".to_string(),
            description: format!(
                "removed {} stale policy ref(s) from {}",
                before - intent.policy_refs.len(),
                intent_ref
            ),
            ref_name: Some(intent_ref),
            target: Some(new_id.to_hex()),
        });
    }
    Ok(applied)
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}
