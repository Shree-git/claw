use std::path::Path;

/// Write text conflict markers (<<<< ==== >>>>)
pub fn write_text_conflict(
    dir: &Path,
    path: &str,
    base: &[u8],
    left: &[u8],
    right: &[u8],
    left_label: &str,
    right_label: &str,
) -> anyhow::Result<()> {
    let file_path = dir.join(path);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let base_str = String::from_utf8_lossy(base);
    let left_str = String::from_utf8_lossy(left);
    let right_str = String::from_utf8_lossy(right);

    // Simple 3-way conflict marker output
    let mut output = String::new();

    let base_lines: Vec<&str> = base_str.lines().collect();
    let left_lines: Vec<&str> = left_str.lines().collect();
    let right_lines: Vec<&str> = right_str.lines().collect();

    // Find common prefix
    let mut i = 0;
    let min_len = base_lines
        .len()
        .min(left_lines.len())
        .min(right_lines.len());
    while i < min_len && left_lines[i] == right_lines[i] && left_lines[i] == base_lines[i] {
        output.push_str(left_lines[i]);
        output.push('\n');
        i += 1;
    }

    // Emit conflict block
    output.push_str(&format!("<<<<<<< {}\n", left_label));
    for line in &left_lines[i..] {
        output.push_str(line);
        output.push('\n');
    }
    output.push_str("=======\n");
    for line in &right_lines[i..] {
        output.push_str(line);
        output.push('\n');
    }
    output.push_str(&format!(">>>>>>> {}\n", right_label));

    std::fs::write(&file_path, output)?;
    Ok(())
}

/// Write JSON conflict (structured)
pub fn write_json_conflict(
    dir: &Path,
    path: &str,
    base: &[u8],
    left: &[u8],
    right: &[u8],
) -> anyhow::Result<()> {
    let file_path = dir.join(path);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conflict = serde_json::json!({
        "_conflict": {
            "base": String::from_utf8_lossy(base),
            "left": String::from_utf8_lossy(left),
            "right": String::from_utf8_lossy(right),
        }
    });
    let content = serde_json::to_string_pretty(&conflict)?;
    std::fs::write(&file_path, content)?;
    Ok(())
}

/// Write binary conflict: main file = left, sidecars for RIGHT and BASE
pub fn write_binary_conflict(
    dir: &Path,
    path: &str,
    base: &[u8],
    left: &[u8],
    right: &[u8],
) -> anyhow::Result<()> {
    let file_path = dir.join(path);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&file_path, left)?;
    std::fs::write(format!("{}.BASE", file_path.display()), base)?;
    std::fs::write(format!("{}.RIGHT", file_path.display()), right)?;
    Ok(())
}
