use std::collections::HashSet;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::ClawStore;

use crate::MergeError;

/// Collect all patches from ancestor to head, walking the revision DAG.
pub fn collect_patches(
    store: &ClawStore,
    ancestor: &ObjectId,
    head: &ObjectId,
) -> Result<Vec<ObjectId>, MergeError> {
    let mut patches = Vec::new();
    let mut visited = HashSet::new();
    collect_recursive(store, head, ancestor, &mut patches, &mut visited)?;
    Ok(patches)
}

fn collect_recursive(
    store: &ClawStore,
    current: &ObjectId,
    stop_at: &ObjectId,
    patches: &mut Vec<ObjectId>,
    visited: &mut HashSet<ObjectId>,
) -> Result<(), MergeError> {
    if current == stop_at || !visited.insert(*current) {
        return Ok(());
    }

    let obj = store.load_object(current)?;
    if let Object::Revision(rev) = obj {
        // Recurse to parents first (topological order)
        for parent in &rev.parents {
            collect_recursive(store, parent, stop_at, patches, visited)?;
        }
        // Then collect this revision's patches
        patches.extend_from_slice(&rev.patches);
    }
    Ok(())
}
