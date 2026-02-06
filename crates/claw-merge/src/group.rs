use std::collections::BTreeMap;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::ClawStore;

use crate::MergeError;

/// Group patches by (target_path, codec_id).
pub fn group_patches(
    store: &ClawStore,
    patch_ids: &[ObjectId],
) -> Result<BTreeMap<(String, String), Vec<ObjectId>>, MergeError> {
    let mut groups: BTreeMap<(String, String), Vec<ObjectId>> = BTreeMap::new();

    for id in patch_ids {
        let obj = store.load_object(id)?;
        if let Object::Patch(patch) = obj {
            let key = (patch.target_path.clone(), patch.codec_id.clone());
            groups.entry(key).or_default().push(*id);
        }
    }

    Ok(groups)
}
