use std::path::{Path, PathBuf};

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

    write_file_atomic(&file_path, output.as_bytes())
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
    write_file_atomic(&file_path, content.as_bytes())
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
    write_file_atomic(&file_path, left)?;
    write_file_atomic(&sidecar_path(&file_path, "BASE"), base)?;
    write_file_atomic(&sidecar_path(&file_path, "RIGHT"), right)?;
    Ok(())
}

pub fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(format!(".{suffix}"));
    PathBuf::from(sidecar)
}

fn write_file_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)?;

    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        use std::io::Write;

        let file = temp.as_file_mut();
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    temp.persist(path).map_err(|err| err.error)?;
    if let Ok(parent_dir) = std::fs::File::open(parent) {
        parent_dir.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_conflict_writes_path_safe_sidecars() {
        let tmp = tempfile::tempdir().unwrap();

        write_binary_conflict(tmp.path(), "nested/archive.bin", b"base", b"left", b"right")
            .unwrap();

        let main_path = tmp.path().join("nested").join("archive.bin");
        assert_eq!(std::fs::read(&main_path).unwrap(), b"left");
        assert_eq!(
            std::fs::read(sidecar_path(&main_path, "BASE")).unwrap(),
            b"base"
        );
        assert_eq!(
            std::fs::read(sidecar_path(&main_path, "RIGHT")).unwrap(),
            b"right"
        );
    }
}
