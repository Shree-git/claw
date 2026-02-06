use claw_core::types::PatchOp;
use similar::{DiffTag, TextDiff};

use crate::codec::Codec;
use crate::PatchError;

pub struct TextLineCodec;

fn context_hash(lines: &[&str], center: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let start = center.saturating_sub(3);
    let end = (center + 4).min(lines.len());
    for line in &lines[start..end] {
        line.hash(&mut hasher);
    }
    hasher.finish()
}

impl Codec for TextLineCodec {
    fn id(&self) -> &str {
        "text/line"
    }

    fn diff(&self, old: &[u8], new: &[u8]) -> Result<Vec<PatchOp>, PatchError> {
        let old_str = std::str::from_utf8(old)
            .map_err(|e| PatchError::ApplyFailed(e.to_string()))?;
        let new_str = std::str::from_utf8(new)
            .map_err(|e| PatchError::ApplyFailed(e.to_string()))?;

        let diff = TextDiff::from_lines(old_str, new_str);
        let old_lines: Vec<&str> = old_str.lines().collect();
        let new_slices = diff.new_slices();
        let old_slices = diff.old_slices();
        let mut ops = Vec::new();
        let mut old_line = 0usize;

        for op in diff.ops() {
            match op.tag() {
                DiffTag::Equal => {
                    old_line = op.old_range().end;
                }
                DiffTag::Delete => {
                    let range = op.old_range();
                    let deleted: String = old_slices[range.start..range.end].join("");
                    ops.push(PatchOp {
                        address: format!("L{}", range.start),
                        op_type: "delete".to_string(),
                        old_data: Some(deleted.as_bytes().to_vec()),
                        new_data: None,
                        context_hash: Some(context_hash(&old_lines, range.start)),
                    });
                    old_line = range.end;
                }
                DiffTag::Insert => {
                    let new_range = op.new_range();
                    let inserted: String = new_slices[new_range.start..new_range.end].join("");
                    ops.push(PatchOp {
                        address: format!("L{}", old_line),
                        op_type: "insert".to_string(),
                        old_data: None,
                        new_data: Some(inserted.as_bytes().to_vec()),
                        context_hash: if !old_lines.is_empty() {
                            Some(context_hash(&old_lines, old_line.min(old_lines.len().saturating_sub(1))))
                        } else {
                            None
                        },
                    });
                }
                DiffTag::Replace => {
                    let old_range = op.old_range();
                    let new_range = op.new_range();
                    let deleted: String = old_slices[old_range.start..old_range.end].join("");
                    let inserted: String = new_slices[new_range.start..new_range.end].join("");
                    ops.push(PatchOp {
                        address: format!("L{}", old_range.start),
                        op_type: "replace".to_string(),
                        old_data: Some(deleted.as_bytes().to_vec()),
                        new_data: Some(inserted.as_bytes().to_vec()),
                        context_hash: Some(context_hash(&old_lines, old_range.start)),
                    });
                    old_line = old_range.end;
                }
            }
        }

        Ok(ops)
    }

    fn apply(&self, base: &[u8], ops: &[PatchOp]) -> Result<Vec<u8>, PatchError> {
        let base_str = std::str::from_utf8(base)
            .map_err(|e| PatchError::ApplyFailed(e.to_string()))?;
        let mut lines: Vec<String> = base_str.lines().map(|l| l.to_string()).collect();
        // Track whether original had trailing newline
        let trailing_newline = base_str.ends_with('\n');

        let mut offset: i64 = 0;

        for op in ops {
            let line_num = parse_line_address(&op.address)?;
            let adjusted = (line_num as i64 + offset) as usize;

            match op.op_type.as_str() {
                "delete" => {
                    let old_data = op.old_data.as_ref()
                        .ok_or_else(|| PatchError::ApplyFailed("delete op missing old_data".into()))?;
                    let old_str = std::str::from_utf8(old_data)
                        .map_err(|e| PatchError::ApplyFailed(e.to_string()))?;
                    let count = old_str.lines().count().max(1);
                    if adjusted + count > lines.len() {
                        return Err(PatchError::ApplyFailed(
                            format!("delete out of bounds: {} + {} > {}", adjusted, count, lines.len()),
                        ));
                    }
                    lines.drain(adjusted..adjusted + count);
                    offset -= count as i64;
                }
                "insert" => {
                    let new_data = op.new_data.as_ref()
                        .ok_or_else(|| PatchError::ApplyFailed("insert op missing new_data".into()))?;
                    let new_str = std::str::from_utf8(new_data)
                        .map_err(|e| PatchError::ApplyFailed(e.to_string()))?;
                    let new_lines: Vec<String> = new_str.lines().map(|l| l.to_string()).collect();
                    let count = new_lines.len();
                    let insert_at = adjusted.min(lines.len());
                    for (i, line) in new_lines.into_iter().enumerate() {
                        lines.insert(insert_at + i, line);
                    }
                    offset += count as i64;
                }
                "replace" => {
                    let old_data = op.old_data.as_ref()
                        .ok_or_else(|| PatchError::ApplyFailed("replace op missing old_data".into()))?;
                    let old_str = std::str::from_utf8(old_data)
                        .map_err(|e| PatchError::ApplyFailed(e.to_string()))?;
                    let del_count = old_str.lines().count().max(1);
                    if adjusted + del_count > lines.len() {
                        return Err(PatchError::ApplyFailed(
                            format!("replace delete out of bounds: {} + {} > {}", adjusted, del_count, lines.len()),
                        ));
                    }
                    lines.drain(adjusted..adjusted + del_count);

                    let new_data = op.new_data.as_ref()
                        .ok_or_else(|| PatchError::ApplyFailed("replace op missing new_data".into()))?;
                    let new_str = std::str::from_utf8(new_data)
                        .map_err(|e| PatchError::ApplyFailed(e.to_string()))?;
                    let new_lines: Vec<String> = new_str.lines().map(|l| l.to_string()).collect();
                    let ins_count = new_lines.len();
                    let insert_at = adjusted.min(lines.len());
                    for (i, line) in new_lines.into_iter().enumerate() {
                        lines.insert(insert_at + i, line);
                    }
                    offset += ins_count as i64 - del_count as i64;
                }
                other => {
                    return Err(PatchError::ApplyFailed(format!("unknown op type: {other}")));
                }
            }
        }

        let mut result = lines.join("\n");
        if trailing_newline || base_str.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
        }
        Ok(result.into_bytes())
    }

    fn invert(&self, ops: &[PatchOp]) -> Result<Vec<PatchOp>, PatchError> {
        let mut inverted: Vec<PatchOp> = ops
            .iter()
            .map(|op| match op.op_type.as_str() {
                "delete" => PatchOp {
                    address: op.address.clone(),
                    op_type: "insert".to_string(),
                    old_data: None,
                    new_data: op.old_data.clone(),
                    context_hash: op.context_hash,
                },
                "insert" => PatchOp {
                    address: op.address.clone(),
                    op_type: "delete".to_string(),
                    old_data: op.new_data.clone(),
                    new_data: None,
                    context_hash: op.context_hash,
                },
                "replace" => PatchOp {
                    address: op.address.clone(),
                    op_type: "replace".to_string(),
                    old_data: op.new_data.clone(),
                    new_data: op.old_data.clone(),
                    context_hash: op.context_hash,
                },
                _ => op.clone(),
            })
            .collect();
        inverted.reverse();
        Ok(inverted)
    }

    fn commute(&self, left: &[PatchOp], right: &[PatchOp]) -> Result<(Vec<PatchOp>, Vec<PatchOp>), PatchError> {
        // Darcs-style commutation: non-overlapping line ranges commute with offset adjustment
        let mut new_right = Vec::new();
        let mut new_left = Vec::new();

        for r_op in right {
            let r_line = parse_line_address(&r_op.address)?;
            let r_count = op_line_count(r_op);

            let mut r_adjusted = r_line as i64;
            let mut can_commute = true;

            for l_op in left {
                let l_line = parse_line_address(&l_op.address)?;
                let l_count = op_line_count(l_op);

                // Check for overlap
                let (l_start, l_end) = match l_op.op_type.as_str() {
                    "delete" | "replace" => (l_line as i64, l_line as i64 + l_count as i64),
                    "insert" => (l_line as i64, l_line as i64),
                    _ => (l_line as i64, l_line as i64),
                };
                let (r_start, r_end) = match r_op.op_type.as_str() {
                    "delete" | "replace" => (r_adjusted, r_adjusted + r_count as i64),
                    "insert" => (r_adjusted, r_adjusted),
                    _ => (r_adjusted, r_adjusted),
                };

                // Check overlap
                if r_start < l_end && r_end > l_start {
                    can_commute = false;
                    break;
                }

                // Adjust offset
                if r_start >= l_end {
                    match l_op.op_type.as_str() {
                        "delete" => r_adjusted -= l_count as i64,
                        "insert" => r_adjusted += l_count as i64,
                        _ => {}
                    }
                }
            }

            if !can_commute {
                return Err(PatchError::CommuteFailed);
            }

            new_right.push(PatchOp {
                address: format!("L{}", r_adjusted),
                ..r_op.clone()
            });
        }

        // Adjust left ops considering right was applied first
        for l_op in left {
            let l_line = parse_line_address(&l_op.address)?;
            let mut l_adjusted = l_line as i64;

            for r_op in &new_right {
                let r_line = parse_line_address(&r_op.address)?;
                let r_count = op_line_count(r_op);

                if (l_adjusted as usize) > r_line {
                    match r_op.op_type.as_str() {
                        "insert" => l_adjusted += r_count as i64,
                        "delete" => l_adjusted -= r_count as i64,
                        _ => {}
                    }
                }
            }

            new_left.push(PatchOp {
                address: format!("L{}", l_adjusted.max(0)),
                ..l_op.clone()
            });
        }

        Ok((new_right, new_left))
    }

    fn merge3(&self, base: &[u8], left: &[u8], right: &[u8]) -> Result<Vec<u8>, PatchError> {
        let base_str = std::str::from_utf8(base)
            .map_err(|e| PatchError::Merge3Failed(e.to_string()))?;
        let left_str = std::str::from_utf8(left)
            .map_err(|e| PatchError::Merge3Failed(e.to_string()))?;
        let right_str = std::str::from_utf8(right)
            .map_err(|e| PatchError::Merge3Failed(e.to_string()))?;

        let base_lines: Vec<&str> = base_str.lines().collect();

        let left_diff = TextDiff::from_lines(base_str, left_str);
        let right_diff = TextDiff::from_lines(base_str, right_str);

        // Build change maps: which base lines were modified
        let mut left_changes: std::collections::HashMap<usize, Vec<&str>> = std::collections::HashMap::new();
        let mut right_changes: std::collections::HashMap<usize, Vec<&str>> = std::collections::HashMap::new();

        collect_changes(&left_diff, &mut left_changes);
        collect_changes(&right_diff, &mut right_changes);

        let mut result = Vec::new();
        let mut i = 0;

        while i < base_lines.len() {
            let left_changed = left_changes.contains_key(&i);
            let right_changed = right_changes.contains_key(&i);

            match (left_changed, right_changed) {
                (false, false) => {
                    result.push(base_lines[i].to_string());
                    i += 1;
                }
                (true, false) => {
                    if let Some(replacement) = left_changes.get(&i) {
                        result.extend(replacement.iter().map(|s| s.to_string()));
                    }
                    i += 1;
                }
                (false, true) => {
                    if let Some(replacement) = right_changes.get(&i) {
                        result.extend(replacement.iter().map(|s| s.to_string()));
                    }
                    i += 1;
                }
                (true, true) => {
                    let left_rep = left_changes.get(&i);
                    let right_rep = right_changes.get(&i);
                    if left_rep == right_rep {
                        if let Some(replacement) = left_rep {
                            result.extend(replacement.iter().map(|s| s.to_string()));
                        }
                    } else {
                        return Err(PatchError::Merge3Failed(
                            format!("conflict at line {i}: both sides changed differently"),
                        ));
                    }
                    i += 1;
                }
            }
        }

        // Handle appended lines
        let max_base = base_lines.len();
        if let Some(appended) = left_changes.get(&max_base) {
            result.extend(appended.iter().map(|s| s.to_string()));
        }
        if let Some(appended) = right_changes.get(&max_base) {
            result.extend(appended.iter().map(|s| s.to_string()));
        }

        let mut output = result.join("\n");
        let left_trailing = left_str.ends_with('\n');
        let right_trailing = right_str.ends_with('\n');
        if (left_trailing || right_trailing) && !output.is_empty() {
            output.push('\n');
        }
        Ok(output.into_bytes())
    }
}

fn collect_changes<'a>(
    diff: &TextDiff<'a, 'a, 'a, str>,
    changes: &mut std::collections::HashMap<usize, Vec<&'a str>>,
) {
    for op in diff.ops() {
        match op.tag() {
            similar::DiffTag::Equal => {}
            similar::DiffTag::Delete | similar::DiffTag::Replace | similar::DiffTag::Insert => {
                let old_range = op.old_range();
                let new_range = op.new_range();
                let new_text: Vec<&str> = diff.new_slices()[new_range.start..new_range.end]
                    .iter()
                    .flat_map(|s| s.lines())
                    .collect();
                let key = old_range.start;
                changes.insert(key, new_text);
            }
        }
    }
}

fn parse_line_address(addr: &str) -> Result<usize, PatchError> {
    addr.strip_prefix('L')
        .and_then(|n| n.parse::<usize>().ok())
        .ok_or_else(|| PatchError::AddressResolutionFailed(format!("invalid line address: {addr}")))
}

fn op_line_count(op: &PatchOp) -> usize {
    match op.op_type.as_str() {
        "delete" | "replace" => {
            if let Some(data) = &op.old_data {
                std::str::from_utf8(data)
                    .map(|s| s.lines().count().max(1))
                    .unwrap_or(1)
            } else {
                1
            }
        }
        "insert" => {
            if let Some(data) = &op.new_data {
                std::str::from_utf8(data)
                    .map(|s| s.lines().count().max(1))
                    .unwrap_or(1)
            } else {
                1
            }
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_and_apply_roundtrip() {
        let codec = TextLineCodec;
        let old = b"line1\nline2\nline3\n";
        let new = b"line1\nmodified\nline3\nextra\n";
        let ops = codec.diff(old, new).unwrap();
        let result = codec.apply(old, &ops).unwrap();
        assert_eq!(result, new);
    }

    #[test]
    fn invert_cancels_patch() {
        let codec = TextLineCodec;
        let old = b"a\nb\nc\n";
        let new = b"a\nx\nc\n";
        let ops = codec.diff(old, new).unwrap();
        let applied = codec.apply(old, &ops).unwrap();
        assert_eq!(applied, new);

        let inv = codec.invert(&ops).unwrap();
        let restored = codec.apply(new, &inv).unwrap();
        assert_eq!(restored, old);
    }

    #[test]
    fn merge3_no_conflict() {
        let codec = TextLineCodec;
        let base = b"line1\nline2\nline3\n";
        let left = b"line1\nleft_change\nline3\n";
        let right = b"line1\nline2\nright_change\n";
        let merged = codec.merge3(base, left, right).unwrap();
        let merged_str = std::str::from_utf8(&merged).unwrap();
        assert!(merged_str.contains("left_change"));
        assert!(merged_str.contains("right_change"));
    }

    #[test]
    fn merge3_conflict() {
        let codec = TextLineCodec;
        let base = b"line1\nline2\nline3\n";
        let left = b"line1\nleft_change\nline3\n";
        let right = b"line1\nright_change\nline3\n";
        assert!(codec.merge3(base, left, right).is_err());
    }
}
