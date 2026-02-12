use std::collections::HashSet;

use claw_core::id::ObjectId;
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
        queue.extend(obj.dependencies());
    }

    visited
}

fn visit_ordered(
    store: &ClawStore,
    id: ObjectId,
    visiting: &mut HashSet<ObjectId>,
    visited: &mut HashSet<ObjectId>,
    out: &mut Vec<ObjectId>,
) {
    if visited.contains(&id) {
        return;
    }
    if !visiting.insert(id) {
        // Defensive cycle guard; object graphs should be acyclic for dependency links.
        tracing::warn!("cycle detected in object dependency traversal at {}", id);
        return;
    }

    let obj = match store.load_object(&id) {
        Ok(obj) => obj,
        Err(e) => {
            tracing::warn!("missing object in dependency traversal: {} ({})", id, e);
            visiting.remove(&id);
            return;
        }
    };

    for dep in obj.dependencies() {
        visit_ordered(store, dep, visiting, visited, out);
    }

    visiting.remove(&id);
    if visited.insert(id) {
        out.push(id);
    }
}

/// Walk the dependency graph from `heads` and return object ids in dependency-first order.
///
/// This ordering is required by transports that validate referenced objects on each insert
/// (for example, ClawLab HTTP object upload), where children must not be sent before parents.
pub fn ordered_reachable_objects(store: &ClawStore, heads: &[ObjectId]) -> Vec<ObjectId> {
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut out = Vec::new();

    for id in heads {
        visit_ordered(store, *id, &mut visiting, &mut visited, &mut out);
    }

    out
}

/// Compute the objects we need to send (have but remote doesn't).
pub fn compute_want_have(
    local_objects: &HashSet<ObjectId>,
    remote_objects: &HashSet<ObjectId>,
) -> (Vec<ObjectId>, Vec<ObjectId>) {
    let want: Vec<ObjectId> = remote_objects.difference(local_objects).copied().collect();
    let have: Vec<ObjectId> = local_objects
        .intersection(remote_objects)
        .copied()
        .collect();
    (want, have)
}
