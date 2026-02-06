use std::path::PathBuf;

/// Find the claw repo root by walking up from the current directory.
pub fn find_repo_root() -> anyhow::Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join(".claw").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            anyhow::bail!("not in a claw repository (no .claw directory found)");
        }
    }
}

pub fn repo_root_or_current() -> PathBuf {
    find_repo_root().unwrap_or_else(|_| std::env::current_dir().unwrap())
}
