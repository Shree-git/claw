use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::{Capsule, Policy, Revision};
use claw_store::ClawStore;

pub(crate) fn resolve_object_ref_or_id(store: &ClawStore, value: &str) -> anyhow::Result<ObjectId> {
    if let Some(id) = store.get_ref(value)? {
        return Ok(id);
    }
    if let Ok(id) = ObjectId::from_hex(value) {
        return Ok(id);
    }
    if let Ok(id) = ObjectId::from_display(value) {
        return Ok(id);
    }
    anyhow::bail!("cannot resolve object or ref: {value}")
}

pub(crate) fn load_revision(
    store: &ClawStore,
    value: &str,
) -> anyhow::Result<(ObjectId, Revision)> {
    let id = resolve_object_ref_or_id(store, value)?;
    match store.load_object(&id)? {
        Object::Revision(revision) => Ok((id, revision)),
        _ => anyhow::bail!("not a revision: {value}"),
    }
}

pub(crate) fn load_capsule(store: &ClawStore, value: &str) -> anyhow::Result<(ObjectId, Capsule)> {
    let id = resolve_object_ref_or_id(store, value)?;
    match store.load_object(&id)? {
        Object::Capsule(capsule) => Ok((id, capsule)),
        Object::Revision(revision) => load_default_capsule(store, &id, &revision),
        _ => anyhow::bail!("not a capsule or revision: {value}"),
    }
}

pub(crate) fn load_default_capsule(
    store: &ClawStore,
    revision_id: &ObjectId,
    revision: &Revision,
) -> anyhow::Result<(ObjectId, Capsule)> {
    if let Some(capsule_id) = revision.capsule_id {
        return load_capsule_id(store, capsule_id, &capsule_id.to_string());
    }

    for ref_name in [
        format!("capsules/by-revision/{}", revision_id.to_hex()),
        format!("capsules/{}", revision_id.to_hex()),
    ] {
        if let Some(capsule_id) = store.get_ref(&ref_name)? {
            return load_capsule_id(store, capsule_id, &ref_name);
        }
    }

    anyhow::bail!(
        "revision {} has no capsule; pass an explicit capsule ref",
        revision_id.to_hex()
    )
}

pub(crate) fn load_capsule_id(
    store: &ClawStore,
    capsule_id: ObjectId,
    source: &str,
) -> anyhow::Result<(ObjectId, Capsule)> {
    match store.load_object(&capsule_id)? {
        Object::Capsule(capsule) => Ok((capsule_id, capsule)),
        _ => anyhow::bail!("not a capsule: {source}"),
    }
}

pub(crate) fn load_policy(
    store: &ClawStore,
    id: &str,
) -> anyhow::Result<(String, ObjectId, Policy)> {
    let ref_name = if id.starts_with("policies/") {
        id.to_string()
    } else {
        format!("policies/{id}")
    };
    let obj_id = store
        .get_ref(&ref_name)?
        .ok_or_else(|| anyhow::anyhow!("policy not found: {ref_name}"))?;
    match store.load_object(&obj_id)? {
        Object::Policy(policy) => Ok((ref_name, obj_id, policy)),
        _ => anyhow::bail!("ref does not point to a policy object: {ref_name}"),
    }
}

pub(crate) fn derive_capsule_trust_score(capsule: &Capsule) -> Option<f32> {
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

pub(crate) fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}
