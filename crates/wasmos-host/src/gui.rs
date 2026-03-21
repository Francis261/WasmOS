use parking_lot::Mutex;
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Default)]
pub struct GuiBroker {
    sessions: Arc<Mutex<BTreeMap<String, GuiSession>>>,
    events: Arc<Mutex<Vec<serde_json::Value>>>,
}

#[derive(Clone, Default, Serialize)]
pub struct GuiSession {
    pub app_id: String,
    pub windows: Vec<WindowDescriptor>,
}

#[derive(Clone, Serialize)]
pub struct WindowDescriptor {
    pub id: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl GuiBroker {
    pub fn session_for(&self, app_id: &str) -> GuiSession {
        let mut sessions = self.sessions.lock();
        sessions
            .entry(app_id.into())
            .or_insert_with(|| GuiSession {
                app_id: app_id.into(),
                windows: Vec::new(),
            })
            .clone()
    }

    pub fn drain_events(&self) -> Vec<serde_json::Value> {
        std::mem::take(&mut *self.events.lock())
    }
}
