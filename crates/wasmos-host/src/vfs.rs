use anyhow::{bail, Result};
use parking_lot::RwLock;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Default)]
pub struct VirtualFileSystem {
    files: Arc<RwLock<BTreeMap<String, Vec<u8>>>>,
    directories: Arc<RwLock<BTreeSet<String>>>,
}

#[derive(Clone, Serialize)]
pub struct DirEntry {
    pub path: String,
    pub kind: &'static str,
    pub size: usize,
}

#[derive(Clone)]
pub struct AppFsView {
    pub root: String,
    pub data: String,
}

impl VirtualFileSystem {
    pub fn bootstrap() -> Self {
        let fs = Self::default();
        for dir in ["/", "/apps", "/data", "/system", "/system/apps"] {
            fs.directories.write().insert(dir.into());
        }
        fs
    }

    pub fn app_view(&self, app_id: &str) -> Result<AppFsView> {
        let root = format!("/apps/{app_id}");
        let data = format!("/data/apps/{app_id}");
        self.ensure_dir(&root);
        self.ensure_dir("/data/apps");
        self.ensure_dir(&data);
        Ok(AppFsView { root, data })
    }

    pub fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>> {
        self.assert_safe(path)?;
        let path = normalize(path);
        let dirs = self.directories.read();
        let files = self.files.read();
        let mut entries = Vec::new();
        for dir in dirs
            .iter()
            .filter(|candidate| is_immediate_child(&path, candidate))
        {
            entries.push(DirEntry {
                path: dir.clone(),
                kind: "dir",
                size: 0,
            });
        }
        for (name, bytes) in files
            .iter()
            .filter(|(candidate, _)| is_immediate_child(&path, candidate))
        {
            entries.push(DirEntry {
                path: name.clone(),
                kind: "file",
                size: bytes.len(),
            });
        }
        Ok(entries)
    }

    pub fn write_file(&self, path: &str, contents: Vec<u8>) -> Result<()> {
        self.assert_safe(path)?;
        let path = normalize(path);
        let parent = parent_dir(&path);
        self.ensure_dir(&parent);
        self.files.write().insert(path, contents);
        Ok(())
    }

    pub fn delete(&self, path: &str) -> Result<()> {
        self.assert_safe(path)?;
        let path = normalize(path);
        if self.files.write().remove(&path).is_some() {
            return Ok(());
        }
        if self.directories.write().remove(&path) {
            return Ok(());
        }
        bail!("path not found")
    }

    fn ensure_dir(&self, path: &str) {
        self.directories.write().insert(normalize(path));
    }

    fn assert_safe(&self, path: &str) -> Result<()> {
        let path = normalize(path);
        if path.contains("..") {
            bail!("directory traversal is forbidden");
        }
        if !(path.starts_with("/data") || path.starts_with("/apps") || path.starts_with("/system"))
        {
            bail!("writes are limited to /data, /apps, or /system");
        }
        Ok(())
    }
}

fn normalize(path: &str) -> String {
    if path == "/" {
        return "/".into();
    }
    format!("/{}", path.trim_matches('/'))
}

fn parent_dir(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(head, _)| if head.is_empty() { "/" } else { head })
        .unwrap_or("/")
        .to_string()
}

fn is_immediate_child(parent: &str, candidate: &str) -> bool {
    if parent == candidate {
        return false;
    }
    let prefix = if parent == "/" {
        "/".to_string()
    } else {
        format!("{parent}/")
    };
    candidate.starts_with(&prefix) && !candidate[prefix.len()..].contains('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denies_path_escape() {
        let fs = VirtualFileSystem::bootstrap();
        assert!(fs.write_file("/../../etc/passwd", vec![]).is_err());
    }

    #[test]
    fn app_view_creates_private_data_folder() {
        let fs = VirtualFileSystem::bootstrap();
        let view = fs.app_view("notes").unwrap();
        assert_eq!(view.data, "/data/apps/notes");
        assert!(fs
            .read_dir("/data/apps")
            .unwrap()
            .iter()
            .any(|entry| entry.path == "/data/apps/notes"));
    }
}
