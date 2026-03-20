use crate::host::HostBridge;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use url::Url;

pub type SocketId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkCapability {
    Http,
    WebSocket,
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: Url,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketPolicy {
    pub allow_remote: bool,
    pub allowed_hosts: Vec<String>,
}

#[derive(Debug)]
pub struct NetworkSubsystem {
    host: Arc<HostBridge>,
    policies: RwLock<BTreeMap<u64, SocketPolicy>>,
}

impl NetworkSubsystem {
    pub fn new(host: Arc<HostBridge>) -> Self {
        Self {
            host,
            policies: RwLock::new(BTreeMap::new()),
        }
    }

    pub async fn set_policy(&self, task_id: u64, policy: SocketPolicy) {
        self.policies.write().await.insert(task_id, policy);
    }

    pub async fn http_request(&self, task_id: u64, request: HttpRequest) -> Result<HttpResponse> {
        self.enforce(task_id, request.url.host_str().unwrap_or_default())
            .await?;
        Ok(HttpResponse {
            status: 501,
            headers: BTreeMap::new(),
            body: b"host HTTP adapter not implemented".to_vec(),
        })
    }

    pub async fn websocket_open(&self, task_id: u64, url: Url) -> Result<SocketId> {
        self.enforce(task_id, url.host_str().unwrap_or_default())
            .await?;
        Ok(task_id << 32 | 1)
    }

    pub async fn tcp_connect(&self, task_id: u64, host: &str, _port: u16) -> Result<SocketId> {
        self.enforce(task_id, host).await?;
        if !self.host.capabilities().tcp {
            bail!("host runtime does not support TCP")
        }
        Ok(task_id << 32 | 2)
    }

    async fn enforce(&self, task_id: u64, host: &str) -> Result<()> {
        let guard = self.policies.read().await;
        if let Some(policy) = guard.get(&task_id) {
            if !policy.allow_remote {
                bail!("network disabled for task {task_id}")
            }
            if !policy.allowed_hosts.is_empty()
                && !policy.allowed_hosts.iter().any(|entry| entry == host)
            {
                bail!("host {host} not permitted")
            }
        }
        Ok(())
    }
}
