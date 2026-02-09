//! Conversion between hand-written Rust types and prost-generated proto types.
//! Used for deterministic Protobuf serialization on disk.

use prost::Message;

use crate::error::CoreError;
use crate::generated::{common as pc, objects as po};
use crate::id::{ChangeId, IntentId, ObjectId};
use crate::object::{Object, TypeTag};
use crate::types::*;

// === Helper conversions ===

fn oid_to_proto(id: &ObjectId) -> pc::ObjectId {
    pc::ObjectId {
        hash: id.as_bytes().to_vec(),
    }
}

fn oid_from_proto(p: &pc::ObjectId) -> Result<ObjectId, CoreError> {
    let arr: [u8; 32] = p
        .hash
        .as_slice()
        .try_into()
        .map_err(|_| CoreError::Deserialization("invalid ObjectId length".into()))?;
    Ok(ObjectId::from_bytes(arr))
}

fn opt_oid_to_proto(id: &Option<ObjectId>) -> Option<pc::ObjectId> {
    id.map(|i| oid_to_proto(&i))
}

fn opt_oid_from_proto(p: &Option<pc::ObjectId>) -> Result<Option<ObjectId>, CoreError> {
    match p {
        Some(o) if !o.hash.is_empty() => Ok(Some(oid_from_proto(o)?)),
        _ => Ok(None),
    }
}

fn ulid_to_proto(id: &IntentId) -> pc::Ulid {
    pc::Ulid {
        data: id.as_bytes().to_vec(),
    }
}

fn ulid_from_proto_intent(p: &pc::Ulid) -> Result<IntentId, CoreError> {
    let arr: [u8; 16] = p
        .data
        .as_slice()
        .try_into()
        .map_err(|_| CoreError::Deserialization("invalid ULID length".into()))?;
    Ok(IntentId::from_bytes(arr))
}

fn change_id_to_proto(id: &ChangeId) -> pc::Ulid {
    pc::Ulid {
        data: id.as_bytes().to_vec(),
    }
}

fn change_id_from_proto(p: &pc::Ulid) -> Result<ChangeId, CoreError> {
    let arr: [u8; 16] = p
        .data
        .as_slice()
        .try_into()
        .map_err(|_| CoreError::Deserialization("invalid ULID length".into()))?;
    Ok(ChangeId::from_bytes(arr))
}

// === Object serialization ===

pub fn serialize_object(obj: &Object) -> Result<Vec<u8>, CoreError> {
    match obj {
        Object::Blob(b) => encode(&blob_to_proto(b)),
        Object::Tree(t) => encode(&tree_to_proto(t)),
        Object::Patch(p) => encode(&patch_to_proto(p)),
        Object::Revision(r) => encode(&revision_to_proto(r)),
        Object::Snapshot(s) => encode(&snapshot_to_proto(s)),
        Object::Intent(i) => encode(&intent_to_proto(i)),
        Object::Change(c) => encode(&change_to_proto(c)),
        Object::Conflict(c) => encode(&conflict_to_proto(c)),
        Object::Capsule(c) => encode(&capsule_to_proto(c)),
        Object::Policy(p) => encode(&policy_to_proto(p)),
        Object::Workstream(w) => encode(&workstream_to_proto(w)),
        Object::RefLog(r) => encode(&reflog_to_proto(r)),
    }
}

pub fn deserialize_object(type_tag: TypeTag, data: &[u8]) -> Result<Object, CoreError> {
    match type_tag {
        TypeTag::Blob => Ok(Object::Blob(blob_from_proto(&decode::<po::Blob>(data)?)?)),
        TypeTag::Tree => Ok(Object::Tree(tree_from_proto(&decode::<po::Tree>(data)?)?)),
        TypeTag::Patch => Ok(Object::Patch(patch_from_proto(&decode::<po::Patch>(
            data,
        )?)?)),
        TypeTag::Revision => Ok(Object::Revision(revision_from_proto(&decode::<
            po::Revision,
        >(data)?)?)),
        TypeTag::Snapshot => Ok(Object::Snapshot(snapshot_from_proto(&decode::<
            po::Snapshot,
        >(data)?)?)),
        TypeTag::Intent => Ok(Object::Intent(intent_from_proto(&decode::<po::Intent>(
            data,
        )?)?)),
        TypeTag::Change => Ok(Object::Change(change_from_proto(&decode::<po::Change>(
            data,
        )?)?)),
        TypeTag::Conflict => Ok(Object::Conflict(conflict_from_proto(&decode::<
            po::Conflict,
        >(data)?)?)),
        TypeTag::Capsule => Ok(Object::Capsule(capsule_from_proto(
            &decode::<po::Capsule>(data)?,
        )?)),
        TypeTag::Policy => Ok(Object::Policy(policy_from_proto(&decode::<po::Policy>(
            data,
        )?)?)),
        TypeTag::Workstream => Ok(Object::Workstream(workstream_from_proto(&decode::<
            po::Workstream,
        >(data)?)?)),
        TypeTag::RefLog => Ok(Object::RefLog(reflog_from_proto(&decode::<po::RefLog>(
            data,
        )?)?)),
    }
}

fn encode<M: Message>(msg: &M) -> Result<Vec<u8>, CoreError> {
    let mut buf = Vec::with_capacity(msg.encoded_len());
    msg.encode(&mut buf)
        .map_err(|e| CoreError::Serialization(e.to_string()))?;
    Ok(buf)
}

fn decode<M: Message + Default>(data: &[u8]) -> Result<M, CoreError> {
    M::decode(data).map_err(|e| CoreError::Deserialization(e.to_string()))
}

// === Blob ===

fn blob_to_proto(b: &Blob) -> po::Blob {
    po::Blob {
        data: b.data.clone(),
        media_type: b.media_type.clone().unwrap_or_default(),
    }
}

fn blob_from_proto(p: &po::Blob) -> Result<Blob, CoreError> {
    Ok(Blob {
        data: p.data.clone(),
        media_type: if p.media_type.is_empty() {
            None
        } else {
            Some(p.media_type.clone())
        },
    })
}

// === Tree ===

fn tree_to_proto(t: &Tree) -> po::Tree {
    po::Tree {
        entries: t
            .entries
            .iter()
            .map(|e| po::TreeEntry {
                name: e.name.clone(),
                mode: match e.mode {
                    FileMode::Regular => 0o100644,
                    FileMode::Executable => 0o100755,
                    FileMode::Symlink => 0o120000,
                    FileMode::Directory => 0o040000,
                },
                object_id: Some(oid_to_proto(&e.object_id)),
            })
            .collect(),
    }
}

fn tree_from_proto(p: &po::Tree) -> Result<Tree, CoreError> {
    let entries = p
        .entries
        .iter()
        .map(|e| {
            let mode = match e.mode {
                0o100755 => FileMode::Executable,
                0o120000 => FileMode::Symlink,
                0o040000 => FileMode::Directory,
                _ => FileMode::Regular,
            };
            let object_id = oid_from_proto(
                e.object_id
                    .as_ref()
                    .ok_or_else(|| CoreError::Deserialization("missing object_id".into()))?,
            )?;
            Ok(TreeEntry {
                name: e.name.clone(),
                mode,
                object_id,
            })
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
    Ok(Tree { entries })
}

// === Patch ===

fn patch_to_proto(p: &Patch) -> po::Patch {
    po::Patch {
        target_path: p.target_path.clone(),
        codec_id: p.codec_id.clone(),
        base_object: opt_oid_to_proto(&p.base_object),
        result_object: opt_oid_to_proto(&p.result_object),
        ops: p
            .ops
            .iter()
            .map(|op| po::PatchOp {
                address: op.address.clone(),
                op_type: op.op_type.clone(),
                old_data: op.old_data.clone().unwrap_or_default(),
                new_data: op.new_data.clone().unwrap_or_default(),
                context_hash: op.context_hash.unwrap_or(0),
            })
            .collect(),
        codec_payload: p.codec_payload.clone().unwrap_or_default(),
    }
}

fn patch_from_proto(p: &po::Patch) -> Result<Patch, CoreError> {
    Ok(Patch {
        target_path: p.target_path.clone(),
        codec_id: p.codec_id.clone(),
        base_object: opt_oid_from_proto(&p.base_object)?,
        result_object: opt_oid_from_proto(&p.result_object)?,
        ops: p
            .ops
            .iter()
            .map(|op| PatchOp {
                address: op.address.clone(),
                op_type: op.op_type.clone(),
                old_data: if op.old_data.is_empty() {
                    None
                } else {
                    Some(op.old_data.clone())
                },
                new_data: if op.new_data.is_empty() {
                    None
                } else {
                    Some(op.new_data.clone())
                },
                context_hash: if op.context_hash == 0 {
                    None
                } else {
                    Some(op.context_hash)
                },
            })
            .collect(),
        codec_payload: if p.codec_payload.is_empty() {
            None
        } else {
            Some(p.codec_payload.clone())
        },
    })
}

// === Revision ===

fn revision_to_proto(r: &Revision) -> po::Revision {
    po::Revision {
        change_id: r
            .change_id
            .map(|c| ulid_to_proto(&IntentId::from_bytes(c.as_bytes()))),
        parents: r.parents.iter().map(oid_to_proto).collect(),
        patches: r.patches.iter().map(oid_to_proto).collect(),
        snapshot_base: opt_oid_to_proto(&r.snapshot_base),
        tree: opt_oid_to_proto(&r.tree),
        capsule_id: opt_oid_to_proto(&r.capsule_id),
        author: r.author.clone(),
        created_at_ms: r.created_at_ms,
        summary: r.summary.clone(),
        policy_evidence: r.policy_evidence.clone(),
    }
}

fn revision_from_proto(p: &po::Revision) -> Result<Revision, CoreError> {
    let change_id = match &p.change_id {
        Some(u) if !u.data.is_empty() => {
            let arr: [u8; 16] = u
                .data
                .as_slice()
                .try_into()
                .map_err(|_| CoreError::Deserialization("invalid change ULID".into()))?;
            Some(ChangeId::from_bytes(arr))
        }
        _ => None,
    };

    Ok(Revision {
        change_id,
        parents: p
            .parents
            .iter()
            .map(oid_from_proto)
            .collect::<Result<_, _>>()?,
        patches: p
            .patches
            .iter()
            .map(oid_from_proto)
            .collect::<Result<_, _>>()?,
        snapshot_base: opt_oid_from_proto(&p.snapshot_base)?,
        tree: opt_oid_from_proto(&p.tree)?,
        capsule_id: opt_oid_from_proto(&p.capsule_id)?,
        author: p.author.clone(),
        created_at_ms: p.created_at_ms,
        summary: p.summary.clone(),
        policy_evidence: p.policy_evidence.clone(),
    })
}

// === Snapshot ===

fn snapshot_to_proto(s: &Snapshot) -> po::Snapshot {
    po::Snapshot {
        tree_root: Some(oid_to_proto(&s.tree_root)),
        revision_id: Some(oid_to_proto(&s.revision_id)),
        created_at_ms: s.created_at_ms,
    }
}

fn snapshot_from_proto(p: &po::Snapshot) -> Result<Snapshot, CoreError> {
    Ok(Snapshot {
        tree_root: oid_from_proto(
            p.tree_root
                .as_ref()
                .ok_or_else(|| CoreError::Deserialization("missing tree_root".into()))?,
        )?,
        revision_id: oid_from_proto(
            p.revision_id
                .as_ref()
                .ok_or_else(|| CoreError::Deserialization("missing revision_id".into()))?,
        )?,
        created_at_ms: p.created_at_ms,
    })
}

// === Intent ===

fn intent_to_proto(i: &Intent) -> po::Intent {
    po::Intent {
        id: Some(ulid_to_proto(&i.id)),
        title: i.title.clone(),
        goal: i.goal.clone(),
        constraints: i.constraints.clone(),
        acceptance_tests: i.acceptance_tests.clone(),
        links: i.links.clone(),
        policy_refs: i.policy_refs.clone(),
        agents: i.agents.clone(),
        change_ids: i.change_ids.clone(),
        depends_on: i.depends_on.clone(),
        supersedes: i.supersedes.clone(),
        status: match i.status {
            IntentStatus::Open => "open".into(),
            IntentStatus::Blocked => "blocked".into(),
            IntentStatus::Done => "done".into(),
            IntentStatus::Superseded => "superseded".into(),
        },
        created_at_ms: i.created_at_ms,
        updated_at_ms: i.updated_at_ms,
    }
}

fn intent_from_proto(p: &po::Intent) -> Result<Intent, CoreError> {
    let id = ulid_from_proto_intent(
        p.id.as_ref()
            .ok_or_else(|| CoreError::Deserialization("missing intent id".into()))?,
    )?;
    let status = match p.status.as_str() {
        "open" => IntentStatus::Open,
        "blocked" => IntentStatus::Blocked,
        "done" => IntentStatus::Done,
        "superseded" => IntentStatus::Superseded,
        _ => IntentStatus::Open,
    };
    Ok(Intent {
        id,
        title: p.title.clone(),
        goal: p.goal.clone(),
        constraints: p.constraints.clone(),
        acceptance_tests: p.acceptance_tests.clone(),
        links: p.links.clone(),
        policy_refs: p.policy_refs.clone(),
        agents: p.agents.clone(),
        change_ids: p.change_ids.clone(),
        depends_on: p.depends_on.clone(),
        supersedes: p.supersedes.clone(),
        status,
        created_at_ms: p.created_at_ms,
        updated_at_ms: p.updated_at_ms,
    })
}

// === Change ===

fn change_to_proto(c: &Change) -> po::Change {
    po::Change {
        id: Some(change_id_to_proto(&c.id)),
        intent_id: Some(ulid_to_proto(&c.intent_id)),
        head_revision: opt_oid_to_proto(&c.head_revision),
        workstream_id: c.workstream_id.clone().unwrap_or_default(),
        status: match c.status {
            ChangeStatus::Open => "open".into(),
            ChangeStatus::Ready => "ready".into(),
            ChangeStatus::Integrated => "integrated".into(),
            ChangeStatus::Abandoned => "abandoned".into(),
        },
        created_at_ms: c.created_at_ms,
        updated_at_ms: c.updated_at_ms,
    }
}

fn change_from_proto(p: &po::Change) -> Result<Change, CoreError> {
    let id = change_id_from_proto(
        p.id.as_ref()
            .ok_or_else(|| CoreError::Deserialization("missing change id".into()))?,
    )?;
    let intent_id = ulid_from_proto_intent(
        p.intent_id
            .as_ref()
            .ok_or_else(|| CoreError::Deserialization("missing intent_id".into()))?,
    )?;
    let status = match p.status.as_str() {
        "open" => ChangeStatus::Open,
        "ready" => ChangeStatus::Ready,
        "integrated" => ChangeStatus::Integrated,
        "abandoned" => ChangeStatus::Abandoned,
        _ => ChangeStatus::Open,
    };
    Ok(Change {
        id,
        intent_id,
        head_revision: opt_oid_from_proto(&p.head_revision)?,
        workstream_id: if p.workstream_id.is_empty() {
            None
        } else {
            Some(p.workstream_id.clone())
        },
        status,
        created_at_ms: p.created_at_ms,
        updated_at_ms: p.updated_at_ms,
    })
}

// === Conflict ===

fn conflict_to_proto(c: &Conflict) -> po::Conflict {
    po::Conflict {
        base_revision: opt_oid_to_proto(&c.base_revision),
        left_revision: Some(oid_to_proto(&c.left_revision)),
        right_revision: Some(oid_to_proto(&c.right_revision)),
        file_path: c.file_path.clone(),
        codec_id: c.codec_id.clone(),
        left_patch_ids: c.left_patch_ids.iter().map(oid_to_proto).collect(),
        right_patch_ids: c.right_patch_ids.iter().map(oid_to_proto).collect(),
        resolution_patch_ids: c.resolution_patch_ids.iter().map(oid_to_proto).collect(),
        status: match c.status {
            ConflictStatus::Open => "open".into(),
            ConflictStatus::Resolved => "resolved".into(),
        },
        created_at_ms: c.created_at_ms,
    }
}

fn conflict_from_proto(p: &po::Conflict) -> Result<Conflict, CoreError> {
    let left_revision = oid_from_proto(
        p.left_revision
            .as_ref()
            .ok_or_else(|| CoreError::Deserialization("missing left_revision".into()))?,
    )?;
    let right_revision = oid_from_proto(
        p.right_revision
            .as_ref()
            .ok_or_else(|| CoreError::Deserialization("missing right_revision".into()))?,
    )?;
    let status = match p.status.as_str() {
        "resolved" => ConflictStatus::Resolved,
        _ => ConflictStatus::Open,
    };
    Ok(Conflict {
        base_revision: opt_oid_from_proto(&p.base_revision)?,
        left_revision,
        right_revision,
        file_path: p.file_path.clone(),
        codec_id: p.codec_id.clone(),
        left_patch_ids: p
            .left_patch_ids
            .iter()
            .map(oid_from_proto)
            .collect::<Result<_, _>>()?,
        right_patch_ids: p
            .right_patch_ids
            .iter()
            .map(oid_from_proto)
            .collect::<Result<_, _>>()?,
        resolution_patch_ids: p
            .resolution_patch_ids
            .iter()
            .map(oid_from_proto)
            .collect::<Result<_, _>>()?,
        status,
        created_at_ms: p.created_at_ms,
    })
}

// === Capsule ===

fn capsule_to_proto(c: &Capsule) -> po::Capsule {
    po::Capsule {
        revision_id: Some(oid_to_proto(&c.revision_id)),
        public_fields: Some(po::CapsulePublic {
            agent_id: c.public_fields.agent_id.clone(),
            agent_version: c.public_fields.agent_version.clone().unwrap_or_default(),
            toolchain_digest: c.public_fields.toolchain_digest.clone().unwrap_or_default(),
            env_fingerprint: c.public_fields.env_fingerprint.clone().unwrap_or_default(),
            evidence: c
                .public_fields
                .evidence
                .iter()
                .map(|e| po::Evidence {
                    name: e.name.clone(),
                    status: e.status.clone(),
                    duration_ms: e.duration_ms,
                    artifact_refs: e.artifact_refs.clone(),
                    summary: e.summary.clone().unwrap_or_default(),
                })
                .collect(),
        }),
        encrypted_private: c.encrypted_private.clone().unwrap_or_default(),
        encryption: c.encryption.clone(),
        key_id: c.key_id.clone().unwrap_or_default(),
        signatures: c
            .signatures
            .iter()
            .map(|s| po::CapsuleSignature {
                signer_id: s.signer_id.clone(),
                signature: s.signature.clone(),
            })
            .collect(),
    }
}

fn capsule_from_proto(p: &po::Capsule) -> Result<Capsule, CoreError> {
    let revision_id = oid_from_proto(
        p.revision_id
            .as_ref()
            .ok_or_else(|| CoreError::Deserialization("missing revision_id".into()))?,
    )?;
    let pub_fields = p
        .public_fields
        .as_ref()
        .ok_or_else(|| CoreError::Deserialization("missing public_fields".into()))?;
    Ok(Capsule {
        revision_id,
        public_fields: CapsulePublic {
            agent_id: pub_fields.agent_id.clone(),
            agent_version: if pub_fields.agent_version.is_empty() {
                None
            } else {
                Some(pub_fields.agent_version.clone())
            },
            toolchain_digest: if pub_fields.toolchain_digest.is_empty() {
                None
            } else {
                Some(pub_fields.toolchain_digest.clone())
            },
            env_fingerprint: if pub_fields.env_fingerprint.is_empty() {
                None
            } else {
                Some(pub_fields.env_fingerprint.clone())
            },
            evidence: pub_fields
                .evidence
                .iter()
                .map(|e| Evidence {
                    name: e.name.clone(),
                    status: e.status.clone(),
                    duration_ms: e.duration_ms,
                    artifact_refs: e.artifact_refs.clone(),
                    summary: if e.summary.is_empty() {
                        None
                    } else {
                        Some(e.summary.clone())
                    },
                })
                .collect(),
        },
        encrypted_private: if p.encrypted_private.is_empty() {
            None
        } else {
            Some(p.encrypted_private.clone())
        },
        encryption: p.encryption.clone(),
        key_id: if p.key_id.is_empty() {
            None
        } else {
            Some(p.key_id.clone())
        },
        signatures: p
            .signatures
            .iter()
            .map(|s| CapsuleSignature {
                signer_id: s.signer_id.clone(),
                signature: s.signature.clone(),
            })
            .collect(),
    })
}

// === Policy ===

fn policy_to_proto(p: &Policy) -> po::Policy {
    po::Policy {
        policy_id: p.policy_id.clone(),
        required_checks: p.required_checks.clone(),
        required_reviewers: p.required_reviewers.clone(),
        sensitive_paths: p.sensitive_paths.clone(),
        quarantine_lane: p.quarantine_lane,
        min_trust_score: p.min_trust_score.clone().unwrap_or_default(),
        visibility: match p.visibility {
            Visibility::Public => "public".into(),
            Visibility::Private => "private".into(),
            Visibility::Restricted => "restricted".into(),
        },
    }
}

fn policy_from_proto(p: &po::Policy) -> Result<Policy, CoreError> {
    let visibility = match p.visibility.as_str() {
        "private" => Visibility::Private,
        "restricted" => Visibility::Restricted,
        _ => Visibility::Public,
    };
    Ok(Policy {
        policy_id: p.policy_id.clone(),
        required_checks: p.required_checks.clone(),
        required_reviewers: p.required_reviewers.clone(),
        sensitive_paths: p.sensitive_paths.clone(),
        quarantine_lane: p.quarantine_lane,
        min_trust_score: if p.min_trust_score.is_empty() {
            None
        } else {
            Some(p.min_trust_score.clone())
        },
        visibility,
    })
}

// === Workstream ===

fn workstream_to_proto(w: &Workstream) -> po::Workstream {
    po::Workstream {
        workstream_id: w.workstream_id.clone(),
        change_stack: w.change_stack.iter().map(change_id_to_proto).collect(),
    }
}

fn workstream_from_proto(p: &po::Workstream) -> Result<Workstream, CoreError> {
    Ok(Workstream {
        workstream_id: p.workstream_id.clone(),
        change_stack: p
            .change_stack
            .iter()
            .map(change_id_from_proto)
            .collect::<Result<_, _>>()?,
    })
}

// === RefLog ===

fn reflog_to_proto(r: &RefLog) -> po::RefLog {
    po::RefLog {
        ref_name: r.ref_name.clone(),
        entries: r
            .entries
            .iter()
            .map(|e| po::RefLogEntry {
                old_target: opt_oid_to_proto(&e.old_target),
                new_target: Some(oid_to_proto(&e.new_target)),
                author: e.author.clone(),
                message: e.message.clone(),
                timestamp: e.timestamp,
            })
            .collect(),
    }
}

fn reflog_from_proto(p: &po::RefLog) -> Result<RefLog, CoreError> {
    let entries = p
        .entries
        .iter()
        .map(|e| {
            let new_target = oid_from_proto(
                e.new_target
                    .as_ref()
                    .ok_or_else(|| CoreError::Deserialization("missing new_target".into()))?,
            )?;
            Ok(RefLogEntry {
                old_target: opt_oid_from_proto(&e.old_target)?,
                new_target,
                author: e.author.clone(),
                message: e.message.clone(),
                timestamp: e.timestamp,
            })
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
    Ok(RefLog {
        ref_name: p.ref_name.clone(),
        entries,
    })
}
