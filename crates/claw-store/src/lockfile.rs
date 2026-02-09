use std::path::{Path, PathBuf};

use crate::StoreError;

pub struct LockFile {
    path: PathBuf,
    _handle: std::fs::File,
}

impl LockFile {
    pub fn acquire(target: &Path) -> Result<Self, StoreError> {
        let lock_path = target.with_extension("lock");
        // Try to create exclusively
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(handle) => Ok(Self {
                path: lock_path,
                _handle: handle,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(StoreError::LockContention(target.to_path_buf()))
            }
            Err(e) => Err(StoreError::Io(e)),
        }
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
