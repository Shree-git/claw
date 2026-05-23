use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::ClawStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapsuleVisibilityFilter {
    Public,
    Private,
    Restricted,
}

impl CapsuleVisibilityFilter {
    pub fn from_proto_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" => None,
            "public" => Some(Self::Public),
            "private" => Some(Self::Private),
            "restricted" => Some(Self::Restricted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PartialCloneFilter {
    pub intent_ids: Vec<String>,
    pub path_prefixes: Vec<String>,
    pub codec_ids: Vec<String>,
    pub time_range: Option<(u64, u64)>,
    pub capsule_visibility: Option<CapsuleVisibilityFilter>,
    pub max_depth: Option<u32>,
    pub max_bytes: Option<u64>,
}

impl PartialCloneFilter {
    fn has_non_visibility_filters(&self) -> bool {
        !self.intent_ids.is_empty()
            || !self.path_prefixes.is_empty()
            || !self.codec_ids.is_empty()
            || self.time_range.is_some()
    }

    fn has_revision_filters(&self) -> bool {
        !self.intent_ids.is_empty()
            || self.time_range.is_some()
            || self.capsule_visibility.is_some()
    }

    fn has_patch_filters(&self) -> bool {
        !self.path_prefixes.is_empty() || !self.codec_ids.is_empty()
    }

    fn intent_matches_change_id(
        &self,
        store: &ClawStore,
        change_id: &claw_core::id::ChangeId,
    ) -> bool {
        let Some(change_obj_id) = store
            .get_ref(&format!("changes/{change_id}"))
            .ok()
            .flatten()
        else {
            return false;
        };

        let Ok(Object::Change(change)) = store.load_object(&change_obj_id) else {
            return false;
        };

        self.intent_ids
            .contains(&change.intent_id.to_string().to_ascii_uppercase())
    }

    fn matches_capsule_visibility(&self, store: &ClawStore, capsule_id: Option<ObjectId>) -> bool {
        let Some(visibility) = self.capsule_visibility else {
            return true;
        };

        let Some(capsule_id) = capsule_id else {
            return matches!(visibility, CapsuleVisibilityFilter::Public);
        };

        let Ok(Object::Capsule(capsule)) = store.load_object(&capsule_id) else {
            return false;
        };

        match visibility {
            CapsuleVisibilityFilter::Public => capsule.encrypted_private.is_none(),
            CapsuleVisibilityFilter::Private | CapsuleVisibilityFilter::Restricted => {
                capsule.encrypted_private.is_some()
            }
        }
    }

    fn matches_patch(&self, patch: &claw_core::types::Patch) -> bool {
        if !self.path_prefixes.is_empty()
            && !self
                .path_prefixes
                .iter()
                .any(|prefix| patch.target_path.starts_with(prefix))
        {
            return false;
        }
        if !self.codec_ids.is_empty() && !self.codec_ids.contains(&patch.codec_id) {
            return false;
        }
        true
    }

    fn revision_matches_patch_filters(&self, store: &ClawStore, patch_ids: &[ObjectId]) -> bool {
        if !self.has_patch_filters() {
            return true;
        }

        patch_ids.iter().any(|patch_id| {
            let Ok(Object::Patch(patch)) = store.load_object(patch_id) else {
                return false;
            };
            self.matches_patch(&patch)
        })
    }

    pub fn matches_object(&self, store: &ClawStore, id: &ObjectId) -> bool {
        let obj = match store.load_object(id) {
            Ok(o) => o,
            Err(_) => return false,
        };

        match &obj {
            Object::Patch(p) => {
                if self.has_revision_filters() {
                    return false;
                }
                self.matches_patch(p)
            }
            Object::Revision(r) => {
                if let Some((start, end)) = self.time_range {
                    if r.created_at_ms < start || r.created_at_ms > end {
                        return false;
                    }
                }

                if !self.intent_ids.is_empty() {
                    let Some(change_id) = &r.change_id else {
                        return false;
                    };
                    if !self.intent_matches_change_id(store, change_id) {
                        return false;
                    }
                }

                if !self.matches_capsule_visibility(store, r.capsule_id) {
                    return false;
                }

                self.revision_matches_patch_filters(store, &r.patches)
            }
            Object::Change(c) => {
                if self.has_patch_filters() || self.time_range.is_some() {
                    return false;
                }
                if !self.intent_ids.is_empty()
                    && !self
                        .intent_ids
                        .contains(&c.intent_id.to_string().to_ascii_uppercase())
                {
                    return false;
                }
                true
            }
            Object::Intent(i) => {
                if self.has_patch_filters() || self.time_range.is_some() {
                    return false;
                }
                if !self.intent_ids.is_empty()
                    && !self
                        .intent_ids
                        .contains(&i.id.to_string().to_ascii_uppercase())
                {
                    return false;
                }
                true
            }
            Object::Capsule(c) => {
                if !self.intent_ids.is_empty()
                    || self.has_patch_filters()
                    || self.time_range.is_some()
                {
                    return false;
                }
                let Some(visibility) = self.capsule_visibility else {
                    return true;
                };

                match visibility {
                    CapsuleVisibilityFilter::Public => c.encrypted_private.is_none(),
                    CapsuleVisibilityFilter::Private | CapsuleVisibilityFilter::Restricted => {
                        c.encrypted_private.is_some()
                    }
                }
            }
            _ => !self.has_non_visibility_filters(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claw_core::id::{ChangeId, IntentId};
    use claw_core::object::Object;
    use claw_core::types::{
        Blob, Capsule, CapsulePublic, Change, ChangeStatus, Intent, IntentStatus, Patch, Revision,
    };

    #[test]
    fn intent_filter_matches_revision_via_change_ref() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ClawStore::init(tmp.path()).unwrap();

        let intent_a = IntentId::new();
        let intent_b = IntentId::new();

        let change_a = Change {
            id: ChangeId::new(),
            intent_id: intent_a,
            head_revision: None,
            workstream_id: None,
            status: ChangeStatus::Open,
            created_at_ms: 1,
            updated_at_ms: 1,
        };
        let change_b = Change {
            id: ChangeId::new(),
            intent_id: intent_b,
            head_revision: None,
            workstream_id: None,
            status: ChangeStatus::Open,
            created_at_ms: 1,
            updated_at_ms: 1,
        };

        let change_a_obj = store
            .store_object(&Object::Change(change_a.clone()))
            .unwrap();
        let change_b_obj = store
            .store_object(&Object::Change(change_b.clone()))
            .unwrap();
        store
            .set_ref(&format!("changes/{}", change_a.id), &change_a_obj)
            .unwrap();
        store
            .set_ref(&format!("changes/{}", change_b.id), &change_b_obj)
            .unwrap();

        let rev_a = Object::Revision(Revision {
            change_id: Some(change_a.id),
            parents: vec![],
            patches: vec![],
            snapshot_base: None,
            tree: None,
            capsule_id: None,
            author: "test".to_string(),
            created_at_ms: 10,
            summary: "a".to_string(),
            policy_evidence: vec![],
        });
        let rev_b = Object::Revision(Revision {
            change_id: Some(change_b.id),
            parents: vec![],
            patches: vec![],
            snapshot_base: None,
            tree: None,
            capsule_id: None,
            author: "test".to_string(),
            created_at_ms: 11,
            summary: "b".to_string(),
            policy_evidence: vec![],
        });

        let rev_a_id = store.store_object(&rev_a).unwrap();
        let rev_b_id = store.store_object(&rev_b).unwrap();

        let filter = PartialCloneFilter {
            intent_ids: vec![intent_a.to_string().to_ascii_uppercase()],
            path_prefixes: vec![],
            codec_ids: vec![],
            time_range: None,
            capsule_visibility: None,
            max_depth: None,
            max_bytes: None,
        };

        assert!(filter.matches_object(&store, &rev_a_id));
        assert!(!filter.matches_object(&store, &rev_b_id));
    }

    #[test]
    fn capsule_visibility_filter_matches_capsules() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ClawStore::init(tmp.path()).unwrap();

        let blob_id = store
            .store_object(&Object::Blob(Blob {
                data: b"rev".to_vec(),
                media_type: None,
            }))
            .unwrap();

        let public_capsule = Object::Capsule(Capsule {
            revision_id: blob_id,
            public_fields: CapsulePublic {
                agent_id: "agent".to_string(),
                agent_version: None,
                toolchain_digest: None,
                env_fingerprint: None,
                evidence: vec![],
            },
            encrypted_private: None,
            encryption: String::new(),
            key_id: None,
            recipients: vec![],
            signatures: vec![],
        });

        let private_capsule = Object::Capsule(Capsule {
            revision_id: blob_id,
            public_fields: CapsulePublic {
                agent_id: "agent".to_string(),
                agent_version: None,
                toolchain_digest: None,
                env_fingerprint: None,
                evidence: vec![],
            },
            encrypted_private: Some(vec![1, 2, 3]),
            encryption: "xchacha20poly1305".to_string(),
            key_id: None,
            recipients: vec![],
            signatures: vec![],
        });

        let public_id = store.store_object(&public_capsule).unwrap();
        let private_id = store.store_object(&private_capsule).unwrap();

        let public_filter = PartialCloneFilter {
            intent_ids: vec![],
            path_prefixes: vec![],
            codec_ids: vec![],
            time_range: None,
            capsule_visibility: Some(CapsuleVisibilityFilter::Public),
            max_depth: None,
            max_bytes: None,
        };

        let private_filter = PartialCloneFilter {
            intent_ids: vec![],
            path_prefixes: vec![],
            codec_ids: vec![],
            time_range: None,
            capsule_visibility: Some(CapsuleVisibilityFilter::Private),
            max_depth: None,
            max_bytes: None,
        };

        assert!(public_filter.matches_object(&store, &public_id));
        assert!(!public_filter.matches_object(&store, &private_id));
        assert!(!private_filter.matches_object(&store, &public_id));
        assert!(private_filter.matches_object(&store, &private_id));

        let intent_obj = Object::Intent(Intent {
            id: IntentId::new(),
            title: "t".to_string(),
            goal: "g".to_string(),
            constraints: vec![],
            acceptance_tests: vec![],
            links: vec![],
            policy_refs: vec![],
            agents: vec![],
            change_ids: vec![],
            depends_on: vec![],
            supersedes: vec![],
            status: IntentStatus::Open,
            created_at_ms: 0,
            updated_at_ms: 0,
        });
        let intent_id = store.store_object(&intent_obj).unwrap();

        assert!(public_filter.matches_object(&store, &intent_id));
        assert!(private_filter.matches_object(&store, &intent_id));
    }

    #[test]
    fn path_filter_matches_revisions_through_patches_without_rooting_unrelated_blobs() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ClawStore::init(tmp.path()).unwrap();

        let old_blob = store
            .store_object(&Object::Blob(Blob {
                data: b"old".to_vec(),
                media_type: None,
            }))
            .unwrap();
        let new_blob = store
            .store_object(&Object::Blob(Blob {
                data: b"new".to_vec(),
                media_type: None,
            }))
            .unwrap();
        let patch_id = store
            .store_object(&Object::Patch(Patch {
                target_path: "src/lib.rs".to_string(),
                codec_id: "text/line".to_string(),
                base_object: Some(old_blob),
                result_object: Some(new_blob),
                ops: vec![],
                codec_payload: None,
            }))
            .unwrap();
        let revision_id = store
            .store_object(&Object::Revision(Revision {
                change_id: None,
                parents: vec![],
                patches: vec![patch_id],
                snapshot_base: None,
                tree: None,
                capsule_id: None,
                author: "test".to_string(),
                created_at_ms: 10,
                summary: "path filter".to_string(),
                policy_evidence: vec![],
            }))
            .unwrap();

        let filter = PartialCloneFilter {
            intent_ids: vec![],
            path_prefixes: vec!["src/".to_string()],
            codec_ids: vec![],
            time_range: None,
            capsule_visibility: None,
            max_depth: None,
            max_bytes: None,
        };

        assert!(filter.matches_object(&store, &revision_id));
        assert!(filter.matches_object(&store, &patch_id));
        assert!(!filter.matches_object(&store, &old_blob));
    }
}
