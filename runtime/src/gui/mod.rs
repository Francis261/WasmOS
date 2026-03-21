use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowSpec {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GuiCommand {
    CreateWindow(WindowSpec),
    DrawText { window_id: u64, x: u32, y: u32, text: String },
    DrawPixels { window_id: u64, width: u32, height: u32, rgba: Vec<u8> },
    CloseWindow { window_id: u64 },
}

#[derive(Default)]
pub struct GuiHost {
    windows: RwLock<HashMap<u64, WindowSpec>>,
}

impl GuiHost {
    pub fn apply(&self, command: GuiCommand) -> Option<u64> {
        let mut windows = self.windows.write().unwrap();
        match command {
            GuiCommand::CreateWindow(spec) => {
                let id = windows.len() as u64 + 1;
                windows.insert(id, spec);
                Some(id)
            }
            GuiCommand::CloseWindow { window_id } => {
                windows.remove(&window_id);
                None
            }
            GuiCommand::DrawText { .. } | GuiCommand::DrawPixels { .. } => None,
        }
    }
}
