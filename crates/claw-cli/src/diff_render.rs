use similar::TextDiff;

pub fn render_unified_diff(path: &str, old_bytes: &[u8], new_bytes: &[u8]) -> String {
    let old_str = String::from_utf8_lossy(old_bytes);
    let new_str = String::from_utf8_lossy(new_bytes);

    let diff = TextDiff::from_lines(old_str.as_ref(), new_str.as_ref());
    let mut output = format!("--- a/{}\n+++ b/{}\n", path, path);
    output.push_str(
        &diff
            .unified_diff()
            .context_radius(3)
            .header(&format!("a/{}", path), &format!("b/{}", path))
            .to_string(),
    );
    output
}

pub fn render_json_diff(path: &str, ops: &[claw_core::types::PatchOp]) -> String {
    let mut output = format!("--- a/{}\n+++ b/{}\n", path, path);
    for op in ops {
        output.push_str(&format!(
            "  {} @{}: ",
            op.op_type, op.address
        ));
        if let Some(old) = &op.old_data {
            output.push_str(&format!(
                "old={:?} ",
                String::from_utf8_lossy(old)
            ));
        }
        if let Some(new) = &op.new_data {
            output.push_str(&format!(
                "new={:?}",
                String::from_utf8_lossy(new)
            ));
        }
        output.push('\n');
    }
    output
}

pub fn render_binary_diff(
    path: &str,
    old_size: usize,
    new_size: usize,
    old_hash: &str,
    new_hash: &str,
) -> String {
    format!(
        "Binary files a/{} and b/{} differ\n  old: {} bytes ({})\n  new: {} bytes ({})\n",
        path,
        path,
        old_size,
        &old_hash[..16.min(old_hash.len())],
        new_size,
        &new_hash[..16.min(new_hash.len())],
    )
}
