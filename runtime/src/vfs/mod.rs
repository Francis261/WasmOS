use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;

use anyhow::{anyhow, Result};

#[derive(Clone, Debug)]
pub enum Node {
    File(Vec<u8>),
    Directory(BTreeMap<String, String>),
}

pub struct VirtualFileSystem {
    nodes: RwLock<HashMap<String, Node>>,
}

impl VirtualFileSystem {
    pub fn new() -> Self {
        let mut nodes = HashMap::new();
        nodes.insert("/".to_string(), Node::Directory(BTreeMap::new()));
        Self { nodes: RwLock::new(nodes) }
    }

    pub fn list(&self, path: &str) -> Result<Vec<String>> {
        let nodes = self.nodes.read().unwrap();
        match nodes.get(path).ok_or_else(|| anyhow!("missing path"))? {
            Node::Directory(entries) => Ok(entries.keys().cloned().collect()),
            Node::File(_) => Err(anyhow!("not a directory")),
        }
    }

    pub fn read_to_string(&self, path: &str) -> Result<String> {
        let nodes = self.nodes.read().unwrap();
        match nodes.get(path).ok_or_else(|| anyhow!("missing path"))? {
            Node::File(bytes) => Ok(String::from_utf8_lossy(bytes).to_string()),
            Node::Directory(_) => Err(anyhow!("is a directory")),
        }
    }

    pub fn write_string(&self, path: &str, content: &str) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap();
        nodes.insert(path.to_string(), Node::File(content.as_bytes().to_vec()));
        Ok(())
    }
}
