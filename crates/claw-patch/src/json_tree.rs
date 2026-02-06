use claw_core::types::PatchOp;
use serde_json::Value;

use crate::codec::Codec;
use crate::PatchError;

pub struct JsonTreeCodec;

impl Codec for JsonTreeCodec {
    fn id(&self) -> &str {
        "json/tree"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        let old_val: Value = serde_json::from_slice(old)
            .map_err(|e| PatchError::InvalidJson(e.to_string()))?;
        let new_val: Value = serde_json::from_slice(new)
            .map_err(|e| PatchError::InvalidJson(e.to_string()))?;

        let mut ops = Vec::new();
        diff_values("", &old_val, &new_val, &mut ops);
        Ok(ops)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        let mut val: Value = serde_json::from_slice(base)
            .map_err(|e| PatchError::InvalidJson(e.to_string()))?;

        for op in ops {
            apply_op(&mut val, op)?;
        }

        serde_json::to_vec_pretty(&val)
            .map_err(|e| PatchError::ApplyFailed(e.to_string()))
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        let mut inverted: Vec<PatchOp> = ops
            .iter()
            .map(|op| match op.op_type.as_str() {
                "insert" => PatchOp {
                    address: op.address.clone(),
                    op_type: "delete".to_string(),
                    old_data: op.new_data.clone(),
                    new_data: None,
                    context_hash: None,
                },
                "delete" => PatchOp {
                    address: op.address.clone(),
                    op_type: "insert".to_string(),
                    old_data: None,
                    new_data: op.old_data.clone(),
                    context_hash: None,
                },
                "replace" => PatchOp {
                    address: op.address.clone(),
                    op_type: "replace".to_string(),
                    old_data: op.new_data.clone(),
                    new_data: op.old_data.clone(),
                    context_hash: None,
                },
                _ => op.clone(),
            })
            .collect();
        inverted.reverse();
        Ok(inverted)
    }

    fn commute(&self, left: &[PatchOp], right: &[PatchOp]) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        // JSON tree commutation: independent paths commute, same path = conflict
        for l in left {
            for r in right {
                let rel = path_relationship(&l.address, &r.address);
                match rel {
                    PathRelation::Equal | PathRelation::AncestorOf | PathRelation::DescendantOf => {
                        return Err(PatchError::CommuteFailed);
                    }
                    PathRelation::Independent => {}
                    PathRelation::SiblingArrayElements => {
                        // Array index adjustment would be needed for full impl
                        // For MVP, treat as non-commutable if they affect the same array
                        return Err(PatchError::CommuteFailed);
                    }
                }
            }
        }
        // Independent paths: they commute as-is
        Ok((right.to_vec(), left.to_vec()))
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        let base_val: Value = serde_json::from_slice(base)
            .map_err(|e| PatchError::InvalidJson(e.to_string()))?;
        let left_val: Value = serde_json::from_slice(left)
            .map_err(|e| PatchError::InvalidJson(e.to_string()))?;
        let right_val: Value = serde_json::from_slice(right)
            .map_err(|e| PatchError::InvalidJson(e.to_string()))?;

        let merged = merge3_values(&base_val, &left_val, &right_val)?;
        serde_json::to_vec_pretty(&merged)
            .map_err(|e| PatchError::Merge3Failed(e.to_string()))
    }
}

fn diff_values(path: &str, old: &Value, new: &Value, ops: &mut Vec<PatchOp>) {
    if old == new {
        return;
    }

    match (old, new) {
        (Value::Object(old_map), Value::Object(new_map)) => {
            // Deleted keys
            for key in old_map.keys() {
                if !new_map.contains_key(key) {
                    let child_path = format!("{path}/{key}");
                    ops.push(PatchOp {
                        address: child_path,
                        op_type: "delete".to_string(),
                        old_data: Some(serde_json::to_vec(&old_map[key]).unwrap()),
                        new_data: None,
                        context_hash: None,
                    });
                }
            }
            // Added keys
            for key in new_map.keys() {
                if !old_map.contains_key(key) {
                    let child_path = format!("{path}/{key}");
                    ops.push(PatchOp {
                        address: child_path,
                        op_type: "insert".to_string(),
                        old_data: None,
                        new_data: Some(serde_json::to_vec(&new_map[key]).unwrap()),
                        context_hash: None,
                    });
                }
            }
            // Changed keys
            for key in old_map.keys() {
                if let Some(new_val) = new_map.get(key) {
                    let child_path = format!("{path}/{key}");
                    diff_values(&child_path, &old_map[key], new_val, ops);
                }
            }
        }
        (Value::Array(old_arr), Value::Array(new_arr)) => {
            // Simple array diff: if lengths differ or elements differ, replace the whole array
            if old_arr.len() != new_arr.len() {
                ops.push(PatchOp {
                    address: path.to_string(),
                    op_type: "replace".to_string(),
                    old_data: Some(serde_json::to_vec(old).unwrap()),
                    new_data: Some(serde_json::to_vec(new).unwrap()),
                    context_hash: None,
                });
            } else {
                for (i, (o, n)) in old_arr.iter().zip(new_arr.iter()).enumerate() {
                    let child_path = format!("{path}/{i}");
                    diff_values(&child_path, o, n, ops);
                }
            }
        }
        _ => {
            // Scalar or type change: replace
            ops.push(PatchOp {
                address: path.to_string(),
                op_type: "replace".to_string(),
                old_data: Some(serde_json::to_vec(old).unwrap()),
                new_data: Some(serde_json::to_vec(new).unwrap()),
                context_hash: None,
            });
        }
    }
}

fn apply_op(val: &mut Value, op: &PatchOp) -> Result<(), PatchError> {
    let parts: Vec<&str> = op.address.split('/').filter(|s| !s.is_empty()).collect();

    if parts.is_empty() {
        // Root replacement
        match op.op_type.as_str() {
            "replace" => {
                let new_val: Value = serde_json::from_slice(
                    op.new_data.as_ref()
                        .ok_or_else(|| PatchError::ApplyFailed("replace missing new_data".into()))?,
                ).map_err(|e| PatchError::ApplyFailed(e.to_string()))?;
                *val = new_val;
            }
            _ => return Err(PatchError::ApplyFailed(format!("unsupported root op: {}", op.op_type))),
        }
        return Ok(());
    }

    // Navigate to parent
    let parent_parts = &parts[..parts.len() - 1];
    let last_key = parts[parts.len() - 1];

    let mut current = val as &mut Value;
    for &part in parent_parts {
        current = navigate_mut(current, part)?;
    }

    match op.op_type.as_str() {
        "insert" => {
            let new_val: Value = serde_json::from_slice(
                op.new_data.as_ref()
                    .ok_or_else(|| PatchError::ApplyFailed("insert missing new_data".into()))?,
            ).map_err(|e| PatchError::ApplyFailed(e.to_string()))?;

            match current {
                Value::Object(map) => {
                    map.insert(last_key.to_string(), new_val);
                }
                Value::Array(arr) => {
                    let idx: usize = last_key.parse()
                        .map_err(|_| PatchError::ApplyFailed("invalid array index".into()))?;
                    if idx > arr.len() {
                        return Err(PatchError::ApplyFailed("array index out of bounds".into()));
                    }
                    arr.insert(idx, new_val);
                }
                _ => return Err(PatchError::ApplyFailed("cannot insert into non-container".into())),
            }
        }
        "delete" => {
            match current {
                Value::Object(map) => {
                    map.remove(last_key);
                }
                Value::Array(arr) => {
                    let idx: usize = last_key.parse()
                        .map_err(|_| PatchError::ApplyFailed("invalid array index".into()))?;
                    if idx >= arr.len() {
                        return Err(PatchError::ApplyFailed("array index out of bounds".into()));
                    }
                    arr.remove(idx);
                }
                _ => return Err(PatchError::ApplyFailed("cannot delete from non-container".into())),
            }
        }
        "replace" => {
            let new_val: Value = serde_json::from_slice(
                op.new_data.as_ref()
                    .ok_or_else(|| PatchError::ApplyFailed("replace missing new_data".into()))?,
            ).map_err(|e| PatchError::ApplyFailed(e.to_string()))?;

            match current {
                Value::Object(map) => {
                    map.insert(last_key.to_string(), new_val);
                }
                Value::Array(arr) => {
                    let idx: usize = last_key.parse()
                        .map_err(|_| PatchError::ApplyFailed("invalid array index".into()))?;
                    if idx >= arr.len() {
                        return Err(PatchError::ApplyFailed("array index out of bounds".into()));
                    }
                    arr[idx] = new_val;
                }
                _ => return Err(PatchError::ApplyFailed("cannot replace in non-container".into())),
            }
        }
        other => return Err(PatchError::ApplyFailed(format!("unknown op type: {other}"))),
    }

    Ok(())
}

fn navigate_mut<'a>(val: &'a mut Value, key: &str) -> Result<&'a mut Value, PatchError> {
    match val {
        Value::Object(map) => map
            .get_mut(key)
            .ok_or_else(|| PatchError::ApplyFailed(format!("key not found: {key}"))),
        Value::Array(arr) => {
            let idx: usize = key
                .parse()
                .map_err(|_| PatchError::ApplyFailed(format!("invalid index: {key}")))?;
            arr.get_mut(idx)
                .ok_or_else(|| PatchError::ApplyFailed(format!("index out of bounds: {idx}")))
        }
        _ => Err(PatchError::ApplyFailed(format!("cannot navigate into scalar at {key}"))),
    }
}

#[derive(Debug, PartialEq)]
enum PathRelation {
    Equal,
    AncestorOf,
    DescendantOf,
    SiblingArrayElements,
    Independent,
}

fn path_relationship(a: &str, b: &str) -> PathRelation {
    if a == b {
        return PathRelation::Equal;
    }
    if b.starts_with(a) && b.as_bytes().get(a.len()) == Some(&b'/') {
        return PathRelation::AncestorOf;
    }
    if a.starts_with(b) && a.as_bytes().get(b.len()) == Some(&b'/') {
        return PathRelation::DescendantOf;
    }
    // Check if siblings in same array
    let a_parts: Vec<&str> = a.split('/').collect();
    let b_parts: Vec<&str> = b.split('/').collect();
    if a_parts.len() == b_parts.len() && a_parts.len() > 1 {
        let a_parent = &a_parts[..a_parts.len() - 1];
        let b_parent = &b_parts[..b_parts.len() - 1];
        if a_parent == b_parent {
            let a_last = a_parts.last().unwrap();
            let b_last = b_parts.last().unwrap();
            if a_last.parse::<usize>().is_ok() && b_last.parse::<usize>().is_ok() {
                return PathRelation::SiblingArrayElements;
            }
        }
    }
    PathRelation::Independent
}

fn merge3_values(base: &Value, left: &Value, right: &Value) -> Result<Value, PatchError> {
    if left == right {
        return Ok(left.clone());
    }
    if left == base {
        return Ok(right.clone());
    }
    if right == base {
        return Ok(left.clone());
    }

    match (base, left, right) {
        (Value::Object(base_map), Value::Object(left_map), Value::Object(right_map)) => {
            let mut merged = serde_json::Map::new();
            let all_keys: std::collections::BTreeSet<&String> = base_map
                .keys()
                .chain(left_map.keys())
                .chain(right_map.keys())
                .collect();

            for key in all_keys {
                let b = base_map.get(key);
                let l = left_map.get(key);
                let r = right_map.get(key);

                match (b, l, r) {
                    (Some(bv), Some(lv), Some(rv)) => {
                        merged.insert(key.clone(), merge3_values(bv, lv, rv)?);
                    }
                    (Some(_), Some(_lv), None) => {
                        // Right deleted, left kept or modified
                        if l == b {
                            // Left didn't change, right deleted - accept deletion
                        } else {
                            // Left modified, right deleted - conflict
                            return Err(PatchError::Merge3Failed(
                                format!("conflict at /{key}: left modified, right deleted"),
                            ));
                        }
                    }
                    (Some(_), None, Some(_rv)) => {
                        if r == b {
                            // Right didn't change, left deleted - accept deletion
                        } else {
                            return Err(PatchError::Merge3Failed(
                                format!("conflict at /{key}: left deleted, right modified"),
                            ));
                        }
                    }
                    (None, Some(lv), Some(rv)) => {
                        // Both added
                        if lv == rv {
                            merged.insert(key.clone(), lv.clone());
                        } else {
                            return Err(PatchError::Merge3Failed(
                                format!("conflict at /{key}: both sides added different values"),
                            ));
                        }
                    }
                    (None, Some(lv), None) => {
                        merged.insert(key.clone(), lv.clone());
                    }
                    (None, None, Some(rv)) => {
                        merged.insert(key.clone(), rv.clone());
                    }
                    (Some(_), None, None) => {
                        // Both deleted - ok
                    }
                    (None, None, None) => unreachable!(),
                }
            }
            Ok(Value::Object(merged))
        }
        _ => {
            // Both changed differently - conflict
            Err(PatchError::Merge3Failed(
                "both sides changed scalar/array value differently".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diff_and_apply_json() {
        let codec = JsonTreeCodec;
        let old = serde_json::to_vec(&json!({"a": 1, "b": 2})).unwrap();
        let new = serde_json::to_vec(&json!({"a": 1, "b": 3, "c": 4})).unwrap();
        let ops = codec.diff(&old, &new).unwrap();
        let result = codec.apply(&old, &ops).unwrap();
        let result_val: Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(result_val, json!({"a": 1, "b": 3, "c": 4}));
    }

    #[test]
    fn json_merge3_no_conflict() {
        let codec = JsonTreeCodec;
        let base = serde_json::to_vec(&json!({"a": 1, "b": 2, "c": 3})).unwrap();
        let left = serde_json::to_vec(&json!({"a": 10, "b": 2, "c": 3})).unwrap();
        let right = serde_json::to_vec(&json!({"a": 1, "b": 20, "c": 3})).unwrap();
        let merged = codec.merge3(&base, &left, &right).unwrap();
        let merged_val: Value = serde_json::from_slice(&merged).unwrap();
        assert_eq!(merged_val, json!({"a": 10, "b": 20, "c": 3}));
    }

    #[test]
    fn json_merge3_both_add_same_key_different_value() {
        let codec = JsonTreeCodec;
        let base = serde_json::to_vec(&json!({"a": 1})).unwrap();
        let left = serde_json::to_vec(&json!({"a": 1, "new": "left"})).unwrap();
        let right = serde_json::to_vec(&json!({"a": 1, "new": "right"})).unwrap();
        assert!(codec.merge3(&base, &left, &right).is_err());
    }

    #[test]
    fn json_invert_roundtrip() {
        let codec = JsonTreeCodec;
        let old = serde_json::to_vec(&json!({"x": 1, "y": 2})).unwrap();
        let new = serde_json::to_vec(&json!({"x": 10, "y": 2, "z": 3})).unwrap();
        let ops = codec.diff(&old, &new).unwrap();
        let applied = codec.apply(&old, &ops).unwrap();
        let applied_val: Value = serde_json::from_slice(&applied).unwrap();
        assert_eq!(applied_val, json!({"x": 10, "y": 2, "z": 3}));

        let inv = codec.invert(&ops).unwrap();
        let restored = codec.apply(&applied, &inv).unwrap();
        let restored_val: Value = serde_json::from_slice(&restored).unwrap();
        assert_eq!(restored_val, json!({"x": 1, "y": 2}));
    }

    #[test]
    fn path_relation_tests() {
        assert_eq!(path_relationship("/a/b", "/a/b"), PathRelation::Equal);
        assert_eq!(path_relationship("/a", "/a/b"), PathRelation::AncestorOf);
        assert_eq!(path_relationship("/a/b", "/a"), PathRelation::DescendantOf);
        assert_eq!(path_relationship("/a/0", "/a/1"), PathRelation::SiblingArrayElements);
        assert_eq!(path_relationship("/a", "/b"), PathRelation::Independent);
    }
}
