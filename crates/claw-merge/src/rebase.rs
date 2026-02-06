use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::PatchOp;
use claw_patch::CodecRegistry;
use claw_store::ClawStore;

use crate::MergeError;

/// Attempt to commute right patches past left patches using the codec's commute operation.
/// Returns the reordered (right', left') patches if successful.
pub fn commute_rebase(
    store: &ClawStore,
    registry: &CodecRegistry,
    codec_id: &str,
    left_ids: &[ObjectId],
    right_ids: &[ObjectId],
) -> Result<(Vec<Vec<PatchOp>>, Vec<Vec<PatchOp>>), MergeError> {
    let codec = registry.get(codec_id)?;

    let mut left_ops: Vec<Vec<PatchOp>> = Vec::new();
    for id in left_ids {
        let obj = store.load_object(id)?;
        if let Object::Patch(p) = obj {
            left_ops.push(p.ops);
        }
    }

    let mut right_ops: Vec<Vec<PatchOp>> = Vec::new();
    for id in right_ids {
        let obj = store.load_object(id)?;
        if let Object::Patch(p) = obj {
            right_ops.push(p.ops);
        }
    }

    // Try to commute each right patch past all left patches
    let mut new_right = Vec::new();
    let mut current_left = left_ops.clone();

    for right in &right_ops {
        let mut commuted_right = right.clone();
        let mut new_left = Vec::new();

        for left in &current_left {
            let (r_prime, l_prime) = codec.commute(left, &commuted_right)?;
            commuted_right = r_prime;
            new_left.push(l_prime);
        }

        new_right.push(commuted_right);
        current_left = new_left;
    }

    Ok((new_right, current_left))
}
