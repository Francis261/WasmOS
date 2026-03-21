use parking_lot::RwLock;
use serde::Serialize;
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone, Default)]
pub struct NetManager {
    allowlists: Arc<RwLock<std::collections::BTreeMap<String, NetPolicy>>>,
}

#[derive(Clone, Serialize)]
pub struct NetPolicy {
    pub app_id: String,
    pub http_hosts: BTreeSet<String>,
    pub websocket_hosts: BTreeSet<String>,
    pub tcp_hosts: BTreeSet<String>,
    pub offline_only: bool,
}

impl NetManager {
    pub fn policy_for(&self, app_id: &str) -> NetPolicy {
        self.allowlists
            .read()
            .get(app_id)
            .cloned()
            .unwrap_or(NetPolicy {
                app_id: app_id.into(),
                http_hosts: BTreeSet::new(),
                websocket_hosts: BTreeSet::new(),
                tcp_hosts: BTreeSet::new(),
                offline_only: true,
            })
    }
}
