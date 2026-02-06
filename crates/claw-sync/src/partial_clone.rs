use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::ClawStore;

pub struct PartialCloneFilter {
    pub path_prefixes: Vec<String>,
    pub codec_ids: Vec<String>,
    pub time_range: Option<(u64, u64)>,
    pub max_depth: Option<u32>,
    pub max_bytes: Option<u64>,
}

impl PartialCloneFilter {
    pub fn matches_object(&self, store: &ClawStore, id: &ObjectId) -> bool {
        let obj = match store.load_object(id) {
            Ok(o) => o,
            Err(_) => return false,
        };

        match &obj {
            Object::Patch(p) => {
                if !self.path_prefixes.is_empty()
                    && !self
                        .path_prefixes
                        .iter()
                        .any(|prefix| p.target_path.starts_with(prefix))
                {
                    return false;
                }
                if !self.codec_ids.is_empty() && !self.codec_ids.contains(&p.codec_id) {
                    return false;
                }
                true
            }
            Object::Revision(r) => {
                if let Some((start, end)) = self.time_range {
                    if r.created_at_ms < start || r.created_at_ms > end {
                        return false;
                    }
                }
                true
            }
            _ => true,
        }
    }
}
