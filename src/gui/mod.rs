use crate::host::HostBridge;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type WindowId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowDescriptor {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DrawCommand {
    Clear {
        rgba: [u8; 4],
    },
    Pixel {
        x: u32,
        y: u32,
        rgba: [u8; 4],
    },
    Text {
        x: u32,
        y: u32,
        text: String,
        rgba: [u8; 4],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GuiEvent {
    KeyDown { key_code: u32 },
    KeyUp { key_code: u32 },
    MouseMove { x: i32, y: i32 },
    MouseClick { x: i32, y: i32, button: u8 },
    CloseRequested,
}

#[derive(Debug)]
pub struct GuiSubsystem {
    _host: Arc<HostBridge>,
    windows: RwLock<BTreeMap<WindowId, WindowDescriptor>>,
}

impl GuiSubsystem {
    pub fn new(host: Arc<HostBridge>) -> Self {
        Self {
            _host: host,
            windows: RwLock::new(BTreeMap::new()),
        }
    }

    pub async fn create_window(&self, task_id: u64, descriptor: WindowDescriptor) -> WindowId {
        let window_id = task_id << 32 | descriptor.width as u64;
        self.windows.write().await.insert(window_id, descriptor);
        window_id
    }

    pub async fn draw(&self, _task_id: u64, _window_id: WindowId, _commands: Vec<DrawCommand>) {
        // Host-specific renderer adapter belongs here.
    }

    pub async fn poll_events(&self, _task_id: u64, _window_id: WindowId) -> Vec<GuiEvent> {
        Vec::new()
    }
}
