use serde::{Deserialize, Serialize};

use crate::id::ObjectId;
use crate::CoreError;

/// File kind and permission mode for a tree entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileMode {
    /// Regular non-executable file.
    Regular,
    /// Executable file.
    Executable,
    /// Symbolic link.
    Symlink,
    /// Directory tree entry.
    Directory,
}

/// A named child entry inside a tree object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeEntry {
    /// Basename of the child entry.
    pub name: String,
    /// File mode for the child.
    pub mode: FileMode,
    /// Object ID referenced by the child.
    pub object_id: ObjectId,
}

/// Directory tree object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tree {
    /// Child entries sorted and validated by callers before persistence.
    pub entries: Vec<TreeEntry>,
}

/// Validate a tree entry basename for canonical object storage.
pub fn validate_tree_entry_name(name: &str) -> Result<(), CoreError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.len() > 255
        || name.ends_with([' ', '.'])
        || has_windows_reserved_char(name)
        || is_windows_reserved_device_name(name)
    {
        return Err(CoreError::InvalidTreeEntryName(name.to_string()));
    }

    if name.chars().any(char::is_control) {
        return Err(CoreError::InvalidTreeEntryName(name.to_string()));
    }

    Ok(())
}

fn has_windows_reserved_char(name: &str) -> bool {
    name.chars()
        .any(|ch| matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
}

fn is_windows_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || is_numbered_windows_device(&upper, "COM")
        || is_numbered_windows_device(&upper, "LPT")
}

fn is_numbered_windows_device(name: &str, prefix: &str) -> bool {
    let Some(suffix) = name.strip_prefix(prefix) else {
        return false;
    };
    matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
}

impl Tree {
    /// Validate entry names and reject duplicate or portable-colliding basenames.
    pub fn validate(&self) -> Result<(), CoreError> {
        let mut seen = std::collections::HashMap::with_capacity(self.entries.len());
        for entry in &self.entries {
            validate_tree_entry_name(&entry.name)?;
            let key = portable_tree_entry_key(&entry.name);
            if let Some(existing) = seen.insert(key, entry.name.as_str()) {
                if existing == entry.name.as_str() {
                    return Err(CoreError::Deserialization(format!(
                        "duplicate tree entry name: {}",
                        entry.name
                    )));
                }
                return Err(CoreError::Deserialization(format!(
                    "tree entry names collide on case-insensitive filesystems: {} and {}",
                    existing, entry.name
                )));
            }
        }
        Ok(())
    }
}

fn portable_tree_entry_key(name: &str) -> String {
    name.chars().flat_map(|ch| ch.to_lowercase()).collect()
}

#[cfg(test)]
mod tests {
    use super::validate_tree_entry_name;

    #[test]
    fn rejects_invalid_tree_entry_names() {
        assert!(validate_tree_entry_name("").is_err());
        assert!(validate_tree_entry_name("..").is_err());
        assert!(validate_tree_entry_name("a/b").is_err());
        assert!(validate_tree_entry_name("a\\b").is_err());
        assert!(validate_tree_entry_name("name:stream").is_err());
        assert!(validate_tree_entry_name("name*glob").is_err());
        assert!(validate_tree_entry_name("trailing-dot.").is_err());
        assert!(validate_tree_entry_name("trailing-space ").is_err());
        assert!(validate_tree_entry_name(&"a".repeat(256)).is_err());
    }

    #[test]
    fn rejects_windows_reserved_device_names() {
        for name in [
            "CON", "con", "CON.txt", "PRN", "AUX", "NUL", "COM1", "com9.log", "LPT1", "lpt9.txt",
        ] {
            assert!(
                validate_tree_entry_name(name).is_err(),
                "reserved device name should be rejected: {name}"
            );
        }
    }

    #[test]
    fn rejects_case_insensitive_duplicate_tree_entry_names() {
        use crate::ObjectId;

        let tree = super::Tree {
            entries: vec![
                super::TreeEntry {
                    name: "README.md".to_string(),
                    mode: super::FileMode::Regular,
                    object_id: ObjectId::from_bytes([0x11; 32]),
                },
                super::TreeEntry {
                    name: "readme.md".to_string(),
                    mode: super::FileMode::Regular,
                    object_id: ObjectId::from_bytes([0x22; 32]),
                },
            ],
        };

        let err = tree
            .validate()
            .expect_err("case-insensitive duplicate tree entries should be rejected");
        assert!(
            err.to_string().contains("case-insensitive filesystems"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn accepts_normal_tree_entry_names() {
        assert!(validate_tree_entry_name("README.md").is_ok());
        assert!(validate_tree_entry_name(".env.example").is_ok());
        assert!(validate_tree_entry_name("src").is_ok());
        assert!(validate_tree_entry_name("notes with spaces.txt").is_ok());
        assert!(validate_tree_entry_name("unicodé-資料.txt").is_ok());
        assert!(validate_tree_entry_name(&"a".repeat(255)).is_ok());
    }
}
