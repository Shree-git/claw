use claw_core::types::PatchOp;

use crate::codec::Codec;
use crate::PatchError;

pub struct BinaryCodec;

impl Codec for BinaryCodec {
    fn id(&self) -> &str {
        "binary"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        if old == new {
            return Ok(vec![]);
        }
        Ok(vec![PatchOp {
            address: "B0".to_string(),
            op_type: "replace".to_string(),
            old_data: Some(old.to_vec()),
            new_data: Some(new.to_vec()),
            context_hash: None,
        }])
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        if ops.is_empty() {
            return Ok(base.to_vec());
        }
        // Return new_data from the last replace op
        for op in ops.iter().rev() {
            if let Some(ref new_data) = op.new_data {
                return Ok(new_data.clone());
            }
        }
        Err(PatchError::ApplyFailed("no replace op found".into()))
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        Ok(ops
            .iter()
            .map(|op| PatchOp {
                address: op.address.clone(),
                op_type: op.op_type.clone(),
                old_data: op.new_data.clone(),
                new_data: op.old_data.clone(),
                context_hash: None,
            })
            .collect())
    }

    fn commute(
        &self,
        _left: &[PatchOp],
        _right: &[PatchOp],
    ) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        Err(PatchError::CommuteFailed)
    }

    fn merge3(&self, _base: &[u8], _left: &[u8], _right: &[u8]) -> Result<Vec<u8>, PatchError> {
        Err(PatchError::Merge3Failed(
            "binary files cannot be auto-merged".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_diff_identical() {
        let codec = BinaryCodec;
        let data = b"hello";
        let ops = codec.diff(data, data).unwrap();
        assert!(ops.is_empty());
    }

    #[test]
    fn binary_diff_and_apply() {
        let codec = BinaryCodec;
        let old = b"old data";
        let new = b"new data";
        let ops = codec.diff(old, new).unwrap();
        assert_eq!(ops.len(), 1);
        let result = codec.apply(old, &ops).unwrap();
        assert_eq!(result, new);
    }

    #[test]
    fn binary_invert() {
        let codec = BinaryCodec;
        let old = b"old";
        let new = b"new";
        let ops = codec.diff(old, new).unwrap();
        let inv = codec.invert(&ops).unwrap();
        let result = codec.apply(new, &inv).unwrap();
        assert_eq!(result, old);
    }

    #[test]
    fn binary_commute_fails() {
        let codec = BinaryCodec;
        let ops = vec![PatchOp {
            address: "B0".to_string(),
            op_type: "replace".to_string(),
            old_data: Some(vec![1]),
            new_data: Some(vec![2]),
            context_hash: None,
        }];
        assert!(codec.commute(&ops, &ops).is_err());
    }

    #[test]
    fn binary_merge3_fails() {
        let codec = BinaryCodec;
        assert!(codec.merge3(b"base", b"left", b"right").is_err());
    }
}
