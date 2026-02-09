use claw_core::types::PatchOp;

use crate::PatchError;

pub trait Codec: Send + Sync {
    fn id(&self) -> &str;

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError>;

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError>;

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError>;

    fn commute(
        &self,
        left: &[PatchOp],
        right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError>;

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError>;
}
