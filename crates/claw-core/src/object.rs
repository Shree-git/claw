use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::id::ObjectId;
use crate::types::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum TypeTag {
    Blob = 0x01,
    Tree = 0x02,
    Patch = 0x03,
    Revision = 0x04,
    Snapshot = 0x05,
    Intent = 0x06,
    Change = 0x07,
    Conflict = 0x08,
    Capsule = 0x09,
    Policy = 0x0A,
    Workstream = 0x0B,
    RefLog = 0x0C,
}

impl TypeTag {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::Blob),
            0x02 => Some(Self::Tree),
            0x03 => Some(Self::Patch),
            0x04 => Some(Self::Revision),
            0x05 => Some(Self::Snapshot),
            0x06 => Some(Self::Intent),
            0x07 => Some(Self::Change),
            0x08 => Some(Self::Conflict),
            0x09 => Some(Self::Capsule),
            0x0A => Some(Self::Policy),
            0x0B => Some(Self::Workstream),
            0x0C => Some(Self::RefLog),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::Tree => "tree",
            Self::Patch => "patch",
            Self::Revision => "revision",
            Self::Snapshot => "snapshot",
            Self::Intent => "intent",
            Self::Change => "change",
            Self::Conflict => "conflict",
            Self::Capsule => "capsule",
            Self::Policy => "policy",
            Self::Workstream => "workstream",
            Self::RefLog => "reflog",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Object {
    Blob(Blob),
    Tree(Tree),
    Patch(Patch),
    Revision(Revision),
    Snapshot(Snapshot),
    Intent(Intent),
    Change(Change),
    Conflict(Conflict),
    Capsule(Capsule),
    Policy(Policy),
    Workstream(Workstream),
    RefLog(RefLog),
}

impl Object {
    pub fn type_tag(&self) -> TypeTag {
        match self {
            Object::Blob(_) => TypeTag::Blob,
            Object::Tree(_) => TypeTag::Tree,
            Object::Patch(_) => TypeTag::Patch,
            Object::Revision(_) => TypeTag::Revision,
            Object::Snapshot(_) => TypeTag::Snapshot,
            Object::Intent(_) => TypeTag::Intent,
            Object::Change(_) => TypeTag::Change,
            Object::Conflict(_) => TypeTag::Conflict,
            Object::Capsule(_) => TypeTag::Capsule,
            Object::Policy(_) => TypeTag::Policy,
            Object::Workstream(_) => TypeTag::Workstream,
            Object::RefLog(_) => TypeTag::RefLog,
        }
    }

    /// Return the set of object IDs referenced by this object.
    ///
    /// This is used by transports to advertise dependency edges (e.g. when
    /// uploading an object graph).
    pub fn dependencies(&self) -> Vec<ObjectId> {
        let mut deps = HashSet::new();

        match self {
            Object::Blob(_) | Object::Intent(_) | Object::Policy(_) | Object::Workstream(_) => {}
            Object::Tree(tree) => {
                for entry in &tree.entries {
                    deps.insert(entry.object_id);
                }
            }
            Object::Patch(patch) => {
                if let Some(id) = patch.base_object {
                    deps.insert(id);
                }
                if let Some(id) = patch.result_object {
                    deps.insert(id);
                }
            }
            Object::Revision(revision) => {
                for parent in &revision.parents {
                    deps.insert(*parent);
                }
                for patch in &revision.patches {
                    deps.insert(*patch);
                }
                if let Some(id) = revision.snapshot_base {
                    deps.insert(id);
                }
                if let Some(id) = revision.tree {
                    deps.insert(id);
                }
                if let Some(id) = revision.capsule_id {
                    deps.insert(id);
                }
            }
            Object::Snapshot(snapshot) => {
                deps.insert(snapshot.tree_root);
                deps.insert(snapshot.revision_id);
            }
            Object::Change(change) => {
                if let Some(id) = change.head_revision {
                    deps.insert(id);
                }
            }
            Object::Conflict(conflict) => {
                if let Some(id) = conflict.base_revision {
                    deps.insert(id);
                }
                deps.insert(conflict.left_revision);
                deps.insert(conflict.right_revision);
                for id in &conflict.left_patch_ids {
                    deps.insert(*id);
                }
                for id in &conflict.right_patch_ids {
                    deps.insert(*id);
                }
                for id in &conflict.resolution_patch_ids {
                    deps.insert(*id);
                }
            }
            Object::Capsule(capsule) => {
                deps.insert(capsule.revision_id);
            }
            Object::RefLog(reflog) => {
                for entry in &reflog.entries {
                    if let Some(old) = entry.old_target {
                        deps.insert(old);
                    }
                    deps.insert(entry.new_target);
                }
            }
        }

        let mut out: Vec<_> = deps.into_iter().collect();
        out.sort_by_key(|id| id.to_hex());
        out
    }

    /// Serialize to deterministic Protobuf encoding.
    pub fn serialize_payload(&self) -> Result<Vec<u8>, crate::CoreError> {
        crate::proto_conv::serialize_object(self)
    }

    /// Deserialize from Protobuf encoding.
    pub fn deserialize_payload(type_tag: TypeTag, data: &[u8]) -> Result<Self, crate::CoreError> {
        crate::proto_conv::deserialize_object(type_tag, data)
    }
}
