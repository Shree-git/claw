use std::path::{Path, PathBuf};

use crate::StoreError;

#[derive(Debug, Clone)]
pub struct RepoLayout {
    root: PathBuf,
}

impl RepoLayout {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn claw_dir(&self) -> PathBuf {
        self.root.join(".claw")
    }

    pub fn objects_dir(&self) -> PathBuf {
        self.claw_dir().join("objects")
    }

    pub fn refs_dir(&self) -> PathBuf {
        self.claw_dir().join("refs")
    }

    pub fn config_file(&self) -> PathBuf {
        self.claw_dir().join("repo.toml")
    }

    pub fn index_file(&self) -> PathBuf {
        self.claw_dir().join("meta.db")
    }

    pub fn packs_dir(&self) -> PathBuf {
        self.claw_dir().join("packs")
    }

    pub fn indices_dir(&self) -> PathBuf {
        self.claw_dir().join("indices")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.claw_dir().join("cache")
    }

    pub fn create_dirs(&self) -> Result<(), StoreError> {
        std::fs::create_dir_all(self.objects_dir())?;
        std::fs::create_dir_all(self.refs_dir().join("changes"))?;
        std::fs::create_dir_all(self.refs_dir().join("intents"))?;
        std::fs::create_dir_all(self.refs_dir().join("workstreams"))?;
        std::fs::create_dir_all(self.refs_dir().join("heads"))?;
        std::fs::create_dir_all(self.packs_dir())?;
        std::fs::create_dir_all(self.indices_dir())?;
        std::fs::create_dir_all(self.cache_dir())?;
        Ok(())
    }
}
