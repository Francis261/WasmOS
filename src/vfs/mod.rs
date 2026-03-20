use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub type FileDescriptor = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsMetadata {
    pub kind: NodeKind,
    pub len: usize,
    pub mapped_host_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsNode {
    pub path: String,
    pub metadata: VfsMetadata,
    pub content: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum VfsError {
    #[error("node not found: {0}")]
    NotFound(String),
    #[error("node already exists: {0}")]
    AlreadyExists(String),
    #[error("not a file: {0}")]
    NotAFile(String),
}

#[derive(Debug, Default)]
pub struct VirtualFileSystem {
    nodes: BTreeMap<String, VfsNode>,
}

impl VirtualFileSystem {
    pub fn new() -> Self {
        let mut vfs = Self::default();
        vfs.create_dir("/").expect("root dir must exist");
        vfs
    }

    pub fn create_dir(&mut self, path: impl Into<String>) -> Result<(), VfsError> {
        let path = normalize(path.into());
        if self.nodes.contains_key(&path) {
            return Err(VfsError::AlreadyExists(path));
        }
        self.nodes.insert(
            path.clone(),
            VfsNode {
                path,
                metadata: VfsMetadata {
                    kind: NodeKind::Directory,
                    len: 0,
                    mapped_host_path: None,
                },
                content: Vec::new(),
            },
        );
        Ok(())
    }

    pub fn write_file(
        &mut self,
        path: impl Into<String>,
        content: Vec<u8>,
    ) -> Result<(), VfsError> {
        let path = normalize(path.into());
        self.nodes.insert(
            path.clone(),
            VfsNode {
                path,
                metadata: VfsMetadata {
                    kind: NodeKind::File,
                    len: content.len(),
                    mapped_host_path: None,
                },
                content,
            },
        );
        Ok(())
    }

    pub fn read_file(&self, path: impl Into<String>) -> Result<Vec<u8>, VfsError> {
        let path = normalize(path.into());
        let node = self
            .nodes
            .get(&path)
            .ok_or_else(|| VfsError::NotFound(path.clone()))?;
        match node.metadata.kind {
            NodeKind::File => Ok(node.content.clone()),
            NodeKind::Directory => Err(VfsError::NotAFile(path)),
        }
    }

    pub fn delete(&mut self, path: impl Into<String>) -> Result<(), VfsError> {
        let path = normalize(path.into());
        self.nodes
            .remove(&path)
            .map(|_| ())
            .ok_or(VfsError::NotFound(path))
    }

    pub fn list_dir(&self, path: impl Into<String>) -> Result<Vec<VfsNode>, VfsError> {
        let path = normalize(path.into());
        if !self.nodes.contains_key(&path) {
            return Err(VfsError::NotFound(path));
        }
        Ok(self
            .nodes
            .iter()
            .filter(|(node_path, _)| is_direct_child(&path, node_path))
            .map(|(_, node)| node.clone())
            .collect())
    }

    pub fn map_host_path(
        &mut self,
        virtual_path: impl Into<String>,
        host_path: impl AsRef<Path>,
    ) -> Result<(), VfsError> {
        let virtual_path = normalize(virtual_path.into());
        let node = self
            .nodes
            .get_mut(&virtual_path)
            .ok_or_else(|| VfsError::NotFound(virtual_path.clone()))?;
        node.metadata.mapped_host_path = Some(host_path.as_ref().to_path_buf());
        Ok(())
    }
}

fn normalize(path: String) -> String {
    let trimmed = path.trim();
    if trimmed == "/" {
        "/".to_string()
    } else {
        format!("/{}", trimmed.trim_start_matches('/'))
    }
}

fn is_direct_child(parent: &str, child: &str) -> bool {
    if parent == child {
        return false;
    }
    let parent = if parent == "/" {
        "/".to_string()
    } else {
        format!("{}/", parent.trim_end_matches('/'))
    };
    let Some(remainder) = child.strip_prefix(&parent) else {
        return false;
    };
    !remainder.is_empty() && !remainder.contains('/')
}
