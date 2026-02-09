use std::collections::HashSet;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::ClawStore;

/// Check if `potential_ancestor` is an ancestor of `descendant` by BFS back from descendant.
pub fn is_ancestor(store: &ClawStore, potential_ancestor: &ObjectId, descendant: &ObjectId) -> bool {
    if potential_ancestor == descendant {
        return true;
    }

    let mut visited = HashSet::new();
    let mut queue = vec![*descendant];

    while let Some(id) = queue.pop() {
        if id == *potential_ancestor {
            return true;
        }
        if !visited.insert(id) {
            continue;
        }
        if let Ok(obj) = store.load_object(&id) {
            if let Object::Revision(rev) = obj {
                queue.extend_from_slice(&rev.parents);
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use claw_core::object::Object;
    use claw_core::types::Revision;

    fn make_rev(store: &ClawStore, parents: Vec<ObjectId>, msg: &str) -> ObjectId {
        let rev = Revision {
            change_id: None,
            parents,
            patches: vec![],
            snapshot_base: None,
            tree: None,
            capsule_id: None,
            author: "test".to_string(),
            created_at_ms: 0,
            summary: msg.to_string(),
            policy_evidence: vec![],
        };
        store.store_object(&Object::Revision(rev)).unwrap()
    }

    #[test]
    fn linear_ancestry() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ClawStore::init(tmp.path()).unwrap();

        let a = make_rev(&store, vec![], "A");
        let b = make_rev(&store, vec![a], "B");
        let c = make_rev(&store, vec![b], "C");

        assert!(is_ancestor(&store, &a, &c));
        assert!(is_ancestor(&store, &b, &c));
        assert!(is_ancestor(&store, &c, &c));
        assert!(!is_ancestor(&store, &c, &a));
    }

    #[test]
    fn diverged_not_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ClawStore::init(tmp.path()).unwrap();

        let a = make_rev(&store, vec![], "A");
        let b = make_rev(&store, vec![a], "B");
        let c = make_rev(&store, vec![a], "C");

        assert!(!is_ancestor(&store, &b, &c));
        assert!(!is_ancestor(&store, &c, &b));
    }

    #[test]
    fn merge_ancestry() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ClawStore::init(tmp.path()).unwrap();

        let a = make_rev(&store, vec![], "A");
        let b = make_rev(&store, vec![a], "B");
        let c = make_rev(&store, vec![a], "C");
        let m = make_rev(&store, vec![b, c], "M");

        assert!(is_ancestor(&store, &a, &m));
        assert!(is_ancestor(&store, &b, &m));
        assert!(is_ancestor(&store, &c, &m));
    }
}
