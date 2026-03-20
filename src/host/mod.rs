use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HostKind {
    Desktop,
    Browser,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostCapabilities {
    pub file_mapping: bool,
    pub tcp: bool,
    pub http: bool,
    pub websocket: bool,
    pub windowing: bool,
}

#[derive(Debug, Clone)]
pub struct HostBridge {
    kind: HostKind,
    capabilities: HostCapabilities,
}

impl HostBridge {
    pub fn detect() -> Self {
        let kind = if cfg!(target_arch = "wasm32") {
            HostKind::Browser
        } else {
            HostKind::Desktop
        };

        Self {
            kind,
            capabilities: HostCapabilities {
                file_mapping: !cfg!(target_arch = "wasm32"),
                tcp: !cfg!(target_arch = "wasm32"),
                http: true,
                websocket: true,
                windowing: true,
            },
        }
    }

    pub fn kind(&self) -> &HostKind {
        &self.kind
    }

    pub fn capabilities(&self) -> &HostCapabilities {
        &self.capabilities
    }
}
