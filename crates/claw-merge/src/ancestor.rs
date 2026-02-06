use std::collections::{HashSet, VecDeque};

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::ClawStore;

use crate::MergeError;

/// Find the lowest common ancestor of two revisions via BFS on the DAG.
pub fn find_lca(
    store: &ClawStore,
    left: &ObjectId,
    right: &ObjectId,
) -> Result<Option<ObjectId>, MergeError> {
    if left == right {
        return Ok(Some(*left));
    }

    let mut left_ancestors: HashSet<ObjectId> = HashSet::new();
    let mut right_ancestors: HashSet<ObjectId> = HashSet::new();
    let mut left_queue: VecDeque<ObjectId> = VecDeque::new();
    let mut right_queue: VecDeque<ObjectId> = VecDeque::new();

    left_ancestors.insert(*left);
    right_ancestors.insert(*right);
    left_queue.push_back(*left);
    right_queue.push_back(*right);

    loop {
        let left_done = left_queue.is_empty();
        let right_done = right_queue.is_empty();

        if left_done && right_done {
            return Ok(None);
        }

        // Expand left
        if let Some(id) = left_queue.pop_front() {
            if right_ancestors.contains(&id) {
                return Ok(Some(id));
            }
            if let Ok(obj) = store.load_object(&id) {
                if let Object::Revision(rev) = obj {
                    for parent in &rev.parents {
                        if left_ancestors.insert(*parent) {
                            left_queue.push_back(*parent);
                        }
                    }
                }
            }
        }

        // Expand right
        if let Some(id) = right_queue.pop_front() {
            if left_ancestors.contains(&id) {
                return Ok(Some(id));
            }
            if let Ok(obj) = store.load_object(&id) {
                if let Object::Revision(rev) = obj {
                    for parent in &rev.parents {
                        if right_ancestors.insert(*parent) {
                            right_queue.push_back(*parent);
                        }
                    }
                }
            }
        }
    }
}
