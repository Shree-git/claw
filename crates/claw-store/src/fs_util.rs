use std::path::Path;

use crate::StoreError;

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().ok_or_else(|| {
        StoreError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no parent: {}", path.display()),
        ))
    })?;
    std::fs::create_dir_all(parent)?;

    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        use std::io::Write;

        let file = temp.as_file_mut();
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    temp.persist(path)
        .map_err(|err| StoreError::Io(err.error))?;
    if let Ok(parent_dir) = std::fs::File::open(parent) {
        parent_dir.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_parent_and_replaces_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested").join("HEAD");

        write_atomic(&path, b"ref: heads/main\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "ref: heads/main\n");

        write_atomic(&path, b"ref: heads/feature\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "ref: heads/feature\n"
        );
    }
}
