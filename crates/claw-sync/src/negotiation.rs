use std::collections::HashSet;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::ClawStore;

/// Walk the revision DAG from `heads` to find all reachable objects.
pub fn find_reachable_objects(store: &ClawStore, heads: &[ObjectId]) -> HashSet<ObjectId> {
    let mut visited = HashSet::new();
    let mut queue: Vec<ObjectId> = heads.to_vec();

    while let Some(id) = queue.pop() {
        if !visited.insert(id) {
            continue;
        }
        let obj = match store.load_object(&id) {
            Ok(obj) => obj,
            Err(e) => {
                tracing::warn!("missing object in DAG traversal: {} ({})", id, e);
                continue;
            }
        };
        match obj {
            Object::Revision(rev) => {
                queue.extend_from_slice(&rev.parents);
                if let Some(tree) = rev.tree {
                    queue.push(tree);
                }
                queue.extend_from_slice(&rev.patches);
            }
            Object::Tree(tree) => {
                for entry in &tree.entries {
                    queue.push(entry.object_id);
                }
            }
            Object::Patch(p) => {
                if let Some(base) = p.base_object {
                    queue.push(base);
                }
                if let Some(result) = p.result_object {
                    queue.push(result);
                }
            }
            Object::Snapshot(s) => {
                queue.push(s.tree_root);
                queue.push(s.revision_id);
            }
            Object::Capsule(c) => {
                queue.push(c.revision_id);
            }
            _ => {}
        }
    }

    visited
}

/// Compute the objects we need to send (have but remote doesn't).
pub fn compute_want_have(
    local_objects: &HashSet<ObjectId>,
    remote_objects: &HashSet<ObjectId>,
) -> (Vec<ObjectId>, Vec<ObjectId>) {
    let want: Vec<ObjectId> = remote_objects
        .difference(local_objects)
        .copied()
        .collect();
    let have: Vec<ObjectId> = local_objects
        .intersection(remote_objects)
        .copied()
        .collect();
    (want, have)
}
