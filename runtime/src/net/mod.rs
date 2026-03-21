use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::RwLock;
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkPolicy {
    pub allow_http: bool,
    pub allow_websocket: bool,
    pub allow_tcp: bool,
    pub allowed_hosts: BTreeSet<String>,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            allow_http: true,
            allow_websocket: false,
            allow_tcp: false,
            allowed_hosts: BTreeSet::new(),
        }
    }
}

#[derive(Default)]
pub struct NetworkController {
    policy: RwLock<NetworkPolicy>,
}

impl NetworkController {
    pub fn set_policy(&self, policy: NetworkPolicy) {
        *self.policy.write().unwrap() = policy;
    }

    pub fn authorize_url(&self, raw: &str) -> Result<Url> {
        let url = Url::parse(raw)?;
        let policy = self.policy.read().unwrap();
        match url.scheme() {
            "http" | "https" if policy.allow_http => {}
            "ws" | "wss" if policy.allow_websocket => {}
            "tcp" if policy.allow_tcp => {}
            _ => return Err(anyhow!("scheme blocked")),
        }
        if !policy.allowed_hosts.is_empty() && !policy.allowed_hosts.contains(url.host_str().unwrap_or_default()) {
            return Err(anyhow!("host blocked"));
        }
        Ok(url)
    }
}
