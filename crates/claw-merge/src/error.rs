use thiserror::Error;

#[derive(Debug, Error)]
pub enum MergeError {
    #[error("no common ancestor found")]
    NoCommonAncestor,
    #[error("store error: {0}")]
    Store(#[from] claw_store::StoreError),
    #[error("patch error: {0}")]
    Patch(#[from] claw_patch::PatchError),
    #[error("core error: {0}")]
    Core(#[from] claw_core::CoreError),
    #[error("merge conflict in {path}: {reason}")]
    Conflict { path: String, reason: String },
}
