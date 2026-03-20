use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub type FileDescriptor = u64;

pub const OPEN_READ: u32 = 1 << 0;
pub const OPEN_WRITE: u32 = 1 << 1;
pub const OPEN_CREATE: u32 = 1 << 2;
pub const OPEN_TRUNCATE: u32 = 1 << 3;
pub const OPEN_APPEND: u32 = 1 << 4;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SeekWhence {
    Start,
    Current,
    End,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsDirEntry {
    pub path: String,
    pub kind: NodeKind,
    pub len: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskVfsPolicy {
    pub allow_read: bool,
    pub allow_write: bool,
    pub allow_create: bool,
    pub allow_delete: bool,
    pub allowed_prefixes: Vec<String>,
}

impl Default for TaskVfsPolicy {
    fn default() -> Self {
        Self {
            allow_read: true,
            allow_write: true,
            allow_create: true,
            allow_delete: false,
            allowed_prefixes: vec!["/".to_string()],
        }
    }
}

#[derive(Debug, Clone)]
struct VfsHandle {
    task_id: u64,
    path: String,
    cursor: usize,
    readable: bool,
    writable: bool,
    append: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsHandleInfo {
    pub fd: FileDescriptor,
    pub path: String,
    pub cursor: usize,
    pub readable: bool,
    pub writable: bool,
    pub append: bool,
}

#[derive(Debug, Error)]
pub enum VfsError {
    #[error("node not found: {0}")]
    NotFound(String),
    #[error("node already exists: {0}")]
    AlreadyExists(String),
    #[error("not a file: {0}")]
    NotAFile(String),
    #[error("not a directory: {0}")]
    NotADirectory(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("invalid file descriptor: {0}")]
    InvalidFd(FileDescriptor),
    #[error("invalid open flags")]
    InvalidOpenFlags,
    #[error("invalid seek")]
    InvalidSeek,
    #[error("snapshot error: {0}")]
    Snapshot(String),
}

#[derive(Debug, Default)]
pub struct VirtualFileSystem {
    nodes: BTreeMap<String, VfsNode>,
    task_policies: BTreeMap<u64, TaskVfsPolicy>,
    handles: BTreeMap<FileDescriptor, VfsHandle>,
    next_fd: FileDescriptor,
}

impl VirtualFileSystem {
    pub fn new() -> Self {
        let mut vfs = Self {
            next_fd: 3,
            ..Self::default()
        };
        vfs.create_dir("/").expect("root dir must exist");
        vfs
    }

    pub fn set_task_policy(&mut self, task_id: u64, policy: TaskVfsPolicy) {
        self.task_policies.insert(task_id, policy);
    }

    pub fn create_dir_for_task(
        &mut self,
        task_id: u64,
        path: impl Into<String>,
    ) -> Result<(), VfsError> {
        let path = normalize(path.into());
        self.enforce(task_id, &path, false, true, false)?;
        self.create_dir(path)
    }

    pub fn delete_for_task(
        &mut self,
        task_id: u64,
        path: impl Into<String>,
    ) -> Result<(), VfsError> {
        let path = normalize(path.into());
        self.enforce(task_id, &path, false, false, true)?;
        self.delete(path)
    }

    pub fn list_dir_for_task(
        &self,
        task_id: u64,
        path: impl Into<String>,
    ) -> Result<Vec<VfsDirEntry>, VfsError> {
        let path = normalize(path.into());
        self.enforce(task_id, &path, true, false, false)?;
        let node = self
            .nodes
            .get(&path)
            .ok_or_else(|| VfsError::NotFound(path.clone()))?;
        if !matches!(node.metadata.kind, NodeKind::Directory) {
            return Err(VfsError::NotADirectory(path));
        }
        Ok(self
            .nodes
            .iter()
            .filter(|(node_path, _)| is_direct_child(&path, node_path))
            .map(|(_, node)| VfsDirEntry {
                path: node.path.clone(),
                kind: node.metadata.kind.clone(),
                len: node.metadata.len,
            })
            .collect())
    }

    pub fn open_for_task(
        &mut self,
        task_id: u64,
        path: impl Into<String>,
        flags: u32,
    ) -> Result<FileDescriptor, VfsError> {
        let path = normalize(path.into());
        let readable = flags & OPEN_READ != 0;
        let writable = flags & OPEN_WRITE != 0 || flags & OPEN_APPEND != 0;
        if !readable && !writable {
            return Err(VfsError::InvalidOpenFlags);
        }

        let wants_create = flags & OPEN_CREATE != 0;
        let wants_truncate = flags & OPEN_TRUNCATE != 0;
        self.enforce(task_id, &path, readable, writable || wants_create, false)?;

        if !self.nodes.contains_key(&path) {
            if wants_create {
                self.write_file(path.clone(), Vec::new())?;
            } else {
                return Err(VfsError::NotFound(path));
            }
        }

        if let Some(node) = self.nodes.get(&path) {
            if !matches!(node.metadata.kind, NodeKind::File) {
                return Err(VfsError::NotAFile(path));
            }
        }

        if wants_truncate {
            let node = self
                .nodes
                .get_mut(&path)
                .ok_or_else(|| VfsError::NotFound(path.clone()))?;
            node.content.clear();
            node.metadata.len = 0;
        }

        let cursor = if flags & OPEN_APPEND != 0 {
            self.nodes
                .get(&path)
                .map(|node| node.content.len())
                .unwrap_or_default()
        } else {
            0
        };

        let fd = self.next_fd;
        self.next_fd = self.next_fd.saturating_add(1);
        self.handles.insert(
            fd,
            VfsHandle {
                task_id,
                path,
                cursor,
                readable,
                writable,
                append: flags & OPEN_APPEND != 0,
            },
        );
        Ok(fd)
    }

    pub fn close_for_task(&mut self, task_id: u64, fd: FileDescriptor) -> Result<(), VfsError> {
        let handle = self.handles.get(&fd).ok_or(VfsError::InvalidFd(fd))?;
        if handle.task_id != task_id {
            return Err(VfsError::PermissionDenied(format!(
                "fd {fd} does not belong to task {task_id}"
            )));
        }
        self.handles.remove(&fd);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn handles_for_task(&self, task_id: u64) -> Vec<VfsHandleInfo> {
        self.handles
            .iter()
            .filter_map(|(fd, handle)| {
                (handle.task_id == task_id).then(|| VfsHandleInfo {
                    fd: *fd,
                    path: handle.path.clone(),
                    cursor: handle.cursor,
                    readable: handle.readable,
                    writable: handle.writable,
                    append: handle.append,
                })
            })
            .collect()
    }

    pub fn save_snapshot(&self, path: impl AsRef<Path>) -> Result<(), VfsError> {
        let snapshot = VfsSnapshot {
            nodes: self.nodes.clone(),
        };
        let payload = serde_json::to_vec_pretty(&snapshot)
            .map_err(|error| VfsError::Snapshot(error.to_string()))?;
        fs::write(path, payload).map_err(|error| VfsError::Snapshot(error.to_string()))?;
        Ok(())
    }

    pub fn load_snapshot(&mut self, path: impl AsRef<Path>) -> Result<(), VfsError> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(());
        }
        let bytes = fs::read(path).map_err(|error| VfsError::Snapshot(error.to_string()))?;
        let snapshot: VfsSnapshot = serde_json::from_slice(&bytes)
            .map_err(|error| VfsError::Snapshot(error.to_string()))?;
        self.nodes = snapshot.nodes;
        if !self.nodes.contains_key("/") {
            self.create_dir("/")?;
        }
        Ok(())
    }

    pub fn read_for_task(
        &mut self,
        task_id: u64,
        fd: FileDescriptor,
        len: usize,
    ) -> Result<Vec<u8>, VfsError> {
        let (path, cursor) = {
            let handle = self.handles.get(&fd).ok_or(VfsError::InvalidFd(fd))?;
            if handle.task_id != task_id || !handle.readable {
                return Err(VfsError::PermissionDenied(format!(
                    "task {task_id} cannot read fd {fd}"
                )));
            }
            (handle.path.clone(), handle.cursor)
        };
        self.enforce(task_id, &path, true, false, false)?;
        let node = self
            .nodes
            .get(&path)
            .ok_or_else(|| VfsError::NotFound(path.clone()))?;
        if !matches!(node.metadata.kind, NodeKind::File) {
            return Err(VfsError::NotAFile(path));
        }
        let start = cursor.min(node.content.len());
        let end = (start + len).min(node.content.len());
        let payload = node.content[start..end].to_vec();

        let handle = self.handles.get_mut(&fd).ok_or(VfsError::InvalidFd(fd))?;
        if handle.task_id != task_id || !handle.readable {
            return Err(VfsError::PermissionDenied(format!(
                "task {task_id} cannot read fd {fd}"
            )));
        }
        handle.cursor = end;
        Ok(payload)
    }

    pub fn write_for_task(
        &mut self,
        task_id: u64,
        fd: FileDescriptor,
        bytes: &[u8],
    ) -> Result<usize, VfsError> {
        let path = {
            let handle = self.handles.get(&fd).ok_or(VfsError::InvalidFd(fd))?;
            if handle.task_id != task_id || !handle.writable {
                return Err(VfsError::PermissionDenied(format!(
                    "task {task_id} cannot write fd {fd}"
                )));
            }
            handle.path.clone()
        };
        self.enforce(task_id, &path, false, true, false)?;

        let (cursor, append) = {
            let handle = self.handles.get(&fd).ok_or(VfsError::InvalidFd(fd))?;
            (handle.cursor, handle.append)
        };

        let node = self
            .nodes
            .get_mut(&path)
            .ok_or_else(|| VfsError::NotFound(path.clone()))?;
        if !matches!(node.metadata.kind, NodeKind::File) {
            return Err(VfsError::NotAFile(path));
        }

        let mut write_cursor = if append { node.content.len() } else { cursor };
        if write_cursor > node.content.len() {
            node.content.resize(write_cursor, 0);
        }

        let required = write_cursor + bytes.len();
        if required > node.content.len() {
            node.content.resize(required, 0);
        }
        node.content[write_cursor..required].copy_from_slice(bytes);
        write_cursor = required;
        node.metadata.len = node.content.len();

        let handle = self.handles.get_mut(&fd).ok_or(VfsError::InvalidFd(fd))?;
        if handle.task_id != task_id || !handle.writable {
            return Err(VfsError::PermissionDenied(format!(
                "task {task_id} cannot write fd {fd}"
            )));
        }
        handle.cursor = write_cursor;
        Ok(bytes.len())
    }

    pub fn seek_for_task(
        &mut self,
        task_id: u64,
        fd: FileDescriptor,
        offset: i64,
        whence: SeekWhence,
    ) -> Result<u64, VfsError> {
        let handle = self.handles.get_mut(&fd).ok_or(VfsError::InvalidFd(fd))?;
        if handle.task_id != task_id {
            return Err(VfsError::PermissionDenied(format!(
                "task {task_id} cannot seek fd {fd}"
            )));
        }
        let file_len = self
            .nodes
            .get(&handle.path)
            .map(|node| node.content.len() as i64)
            .ok_or_else(|| VfsError::NotFound(handle.path.clone()))?;

        let base = match whence {
            SeekWhence::Start => 0,
            SeekWhence::Current => handle.cursor as i64,
            SeekWhence::End => file_len,
        };
        let new_offset = base.checked_add(offset).ok_or(VfsError::InvalidSeek)?;
        if new_offset < 0 {
            return Err(VfsError::InvalidSeek);
        }
        handle.cursor = new_offset as usize;
        Ok(handle.cursor as u64)
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

    fn enforce(
        &self,
        task_id: u64,
        path: &str,
        need_read: bool,
        need_write_or_create: bool,
        need_delete: bool,
    ) -> Result<(), VfsError> {
        let policy = self
            .task_policies
            .get(&task_id)
            .cloned()
            .unwrap_or_default();
        if need_read && !policy.allow_read {
            return Err(VfsError::PermissionDenied(format!(
                "task {task_id} cannot read"
            )));
        }
        if need_write_or_create && !(policy.allow_write || policy.allow_create) {
            return Err(VfsError::PermissionDenied(format!(
                "task {task_id} cannot write/create"
            )));
        }
        if need_delete && !policy.allow_delete {
            return Err(VfsError::PermissionDenied(format!(
                "task {task_id} cannot delete"
            )));
        }
        if !policy.allowed_prefixes.is_empty()
            && !policy.allowed_prefixes.iter().any(|prefix| {
                path == prefix || path.starts_with(&format!("{}/", prefix.trim_end_matches('/')))
            })
        {
            return Err(VfsError::PermissionDenied(format!(
                "path {path} not permitted"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VfsSnapshot {
    nodes: BTreeMap<String, VfsNode>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vfs_policy_denies_write_when_disabled() {
        let mut vfs = VirtualFileSystem::new();
        let task_id = 7;
        vfs.set_task_policy(
            task_id,
            TaskVfsPolicy {
                allow_read: true,
                allow_write: false,
                allow_create: false,
                allow_delete: false,
                allowed_prefixes: vec!["/sandbox".to_string()],
            },
        );
        vfs.create_dir("/sandbox").expect("sandbox should exist");

        let err = vfs
            .open_for_task(task_id, "/sandbox/file.txt", OPEN_WRITE | OPEN_CREATE)
            .expect_err("write+create should be rejected");
        assert!(
            matches!(err, VfsError::PermissionDenied(_)),
            "expected permission denied, got {err:?}"
        );
    }

    #[test]
    fn vfs_handle_roundtrip_and_fd_isolation() {
        let mut vfs = VirtualFileSystem::new();
        let task_a = 11;
        let task_b = 12;
        let fd = vfs
            .open_for_task(task_a, "/notes.txt", OPEN_READ | OPEN_WRITE | OPEN_CREATE)
            .expect("task A should create file");
        vfs.write_for_task(task_a, fd, b"hello")
            .expect("task A write should succeed");

        let err = vfs
            .read_for_task(task_b, fd, 5)
            .expect_err("task B should not use task A's fd");
        assert!(
            matches!(err, VfsError::PermissionDenied(_)),
            "expected permission denied for foreign fd, got {err:?}"
        );

        vfs.seek_for_task(task_a, fd, 0, SeekWhence::Start)
            .expect("seek should succeed");
        let data = vfs
            .read_for_task(task_a, fd, 5)
            .expect("task A read should succeed");
        assert_eq!(data, b"hello");
    }

    #[test]
    fn snapshot_roundtrip_restores_nodes() {
        let snapshot_file = std::env::temp_dir().join("wasmos_vfs_snapshot_test.json");
        let mut vfs = VirtualFileSystem::new();
        vfs.create_dir("/persist")
            .expect("directory should be created");
        vfs.write_file("/persist/hello.txt", b"world".to_vec())
            .expect("file should be created");
        vfs.save_snapshot(&snapshot_file)
            .expect("snapshot save should succeed");

        let mut restored = VirtualFileSystem::new();
        restored
            .load_snapshot(&snapshot_file)
            .expect("snapshot load should succeed");
        let bytes = restored
            .read_file("/persist/hello.txt")
            .expect("restored file should exist");
        assert_eq!(bytes, b"world");
    }
}
