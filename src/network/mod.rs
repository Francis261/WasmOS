use crate::host::HostBridge;
use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{Duration, timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use url::Url;

pub type SocketId = u64;

type SharedWebSocket = Arc<Mutex<WebSocketStream<MaybeTlsStream<TcpStream>>>>;
type SharedTcp = Arc<Mutex<TcpStream>>;

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
    pub allow_http: bool,
    pub allow_websocket: bool,
    pub allow_tcp: bool,
    pub allowed_hosts: Vec<String>,
    pub denied_hosts: Vec<String>,
    pub dns_allowlist: Vec<String>,
    pub connect_timeout_ms: u64,
    pub io_timeout_ms: u64,
    pub max_response_bytes: usize,
    pub max_message_bytes: usize,
}

impl Default for SocketPolicy {
    fn default() -> Self {
        Self {
            allow_remote: false,
            allow_http: false,
            allow_websocket: false,
            allow_tcp: false,
            allowed_hosts: Vec::new(),
            denied_hosts: Vec::new(),
            dns_allowlist: Vec::new(),
            connect_timeout_ms: 5_000,
            io_timeout_ms: 10_000,
            max_response_bytes: 256 * 1024,
            max_message_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug)]
struct WebSocketSession {
    task_id: u64,
    stream: SharedWebSocket,
    max_message_bytes: usize,
}

#[derive(Debug)]
#[allow(dead_code)]
struct TcpSession {
    task_id: u64,
    stream: SharedTcp,
    io_timeout_ms: u64,
    max_message_bytes: usize,
}

#[derive(Debug)]
pub struct NetworkSubsystem {
    host: Arc<HostBridge>,
    policies: RwLock<BTreeMap<u64, SocketPolicy>>,
    last_errors: RwLock<BTreeMap<u64, String>>,
    next_socket_id: Mutex<SocketId>,
    websockets: RwLock<BTreeMap<SocketId, WebSocketSession>>,
    tcp_sockets: RwLock<BTreeMap<SocketId, TcpSession>>,
}

impl NetworkSubsystem {
    pub fn new(host: Arc<HostBridge>) -> Self {
        Self {
            host,
            policies: RwLock::new(BTreeMap::new()),
            last_errors: RwLock::new(BTreeMap::new()),
            next_socket_id: Mutex::new(1),
            websockets: RwLock::new(BTreeMap::new()),
            tcp_sockets: RwLock::new(BTreeMap::new()),
        }
    }

    pub async fn set_policy(&self, task_id: u64, policy: SocketPolicy) {
        self.policies.write().await.insert(task_id, policy);
    }

    pub async fn policy(&self, task_id: u64) -> SocketPolicy {
        self.policy_for(task_id).await
    }

    pub async fn last_error(&self, task_id: u64) -> Option<String> {
        self.last_errors.read().await.get(&task_id).cloned()
    }

    pub async fn http_request(&self, task_id: u64, request: HttpRequest) -> Result<HttpResponse> {
        self.enforce(
            task_id,
            request.url.host_str().unwrap_or_default(),
            NetworkCapability::Http,
        )
        .await?;
        if !self.host.capabilities().http {
            bail!("host runtime does not support HTTP")
        }

        let policy = self.policy_for(task_id).await;
        let connect_timeout = Duration::from_millis(policy.connect_timeout_ms);
        let client = Client::builder().connect_timeout(connect_timeout).build()?;

        let mut builder = client
            .request(
                request
                    .method
                    .parse::<reqwest::Method>()
                    .context("invalid HTTP method")?,
                request.url,
            )
            .body(request.body);

        for (key, value) in request.headers {
            builder = builder.header(key, value);
        }

        let response =
            match timeout(Duration::from_millis(policy.io_timeout_ms), builder.send()).await {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => {
                    self.record_error(task_id, format!("HTTP request failed: {error}"))
                        .await;
                    return Err(error.into());
                }
                Err(error) => {
                    let message = format!("HTTP request timed out: {error}");
                    self.record_error(task_id, message.clone()).await;
                    return Err(error.into());
                }
            };
        let status = response.status().as_u16();
        let mut headers = BTreeMap::new();
        for (key, value) in response.headers() {
            headers.insert(
                key.to_string(),
                value.to_str().unwrap_or_default().to_string(),
            );
        }
        let bytes = match timeout(
            Duration::from_millis(policy.io_timeout_ms),
            response.bytes(),
        )
        .await
        {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error)) => {
                self.record_error(task_id, format!("HTTP body read failed: {error}"))
                    .await;
                return Err(error.into());
            }
            Err(error) => {
                let message = format!("HTTP body read timed out: {error}");
                self.record_error(task_id, message.clone()).await;
                return Err(error.into());
            }
        };
        if bytes.len() > policy.max_response_bytes {
            let message = format!("HTTP response exceeded {} bytes", policy.max_response_bytes);
            self.record_error(task_id, message.clone()).await;
            bail!("{message}")
        }

        self.clear_error(task_id).await;
        Ok(HttpResponse {
            status,
            headers,
            body: bytes.to_vec(),
        })
    }

    pub async fn websocket_open(&self, task_id: u64, url: Url) -> Result<SocketId> {
        self.enforce(
            task_id,
            url.host_str().unwrap_or_default(),
            NetworkCapability::WebSocket,
        )
        .await?;
        if !self.host.capabilities().websocket {
            bail!("host runtime does not support WebSocket")
        }
        let policy = self.policy_for(task_id).await;
        let (stream, _) = timeout(
            Duration::from_millis(policy.connect_timeout_ms),
            connect_async(url.as_str()),
        )
        .await
        .map_err(|error| anyhow::anyhow!("websocket connect timed out: {error}"))?
        .map_err(|error| anyhow::anyhow!("websocket connect failed: {error}"))?;

        let socket_id = self.next_socket_id().await;
        self.websockets.write().await.insert(
            socket_id,
            WebSocketSession {
                task_id,
                stream: Arc::new(Mutex::new(stream)),
                max_message_bytes: policy.max_message_bytes,
            },
        );
        Ok(socket_id)
    }

    pub async fn websocket_send_text(
        &self,
        task_id: u64,
        socket_id: SocketId,
        text: String,
    ) -> Result<()> {
        let sockets = self.websockets.read().await;
        let session = sockets.get(&socket_id).context("unknown websocket id")?;
        if session.task_id != task_id {
            bail!("task {task_id} cannot use websocket {socket_id}")
        }
        if text.len() > session.max_message_bytes {
            bail!(
                "websocket payload exceeded {} bytes",
                session.max_message_bytes
            )
        }
        let mut stream = session.stream.lock().await;
        stream
            .send(tokio_tungstenite::tungstenite::Message::Text(text))
            .await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn websocket_recv_text(&self, task_id: u64, socket_id: SocketId) -> Result<String> {
        let sockets = self.websockets.read().await;
        let session = sockets.get(&socket_id).context("unknown websocket id")?;
        if session.task_id != task_id {
            bail!("task {task_id} cannot use websocket {socket_id}")
        }
        let mut stream = session.stream.lock().await;
        let message = stream.next().await.context("websocket closed")??;
        let text = match message {
            tokio_tungstenite::tungstenite::Message::Text(text) => text,
            other => other.into_text()?,
        };
        if text.len() > session.max_message_bytes {
            bail!(
                "websocket message exceeded {} bytes",
                session.max_message_bytes
            )
        }
        Ok(text)
    }

    pub async fn tcp_connect(&self, task_id: u64, host: &str, port: u16) -> Result<SocketId> {
        self.enforce(task_id, host, NetworkCapability::Tcp).await?;
        if !self.host.capabilities().tcp {
            bail!("host runtime does not support TCP")
        }
        let policy = self.policy_for(task_id).await;
        let stream = timeout(
            Duration::from_millis(policy.connect_timeout_ms),
            TcpStream::connect((host, port)),
        )
        .await
        .map_err(|error| anyhow::anyhow!("tcp connect timed out: {error}"))?
        .map_err(|error| anyhow::anyhow!("tcp connect failed: {error}"))?;

        let socket_id = self.next_socket_id().await;
        self.tcp_sockets.write().await.insert(
            socket_id,
            TcpSession {
                task_id,
                stream: Arc::new(Mutex::new(stream)),
                io_timeout_ms: policy.io_timeout_ms,
                max_message_bytes: policy.max_message_bytes,
            },
        );
        Ok(socket_id)
    }

    #[allow(dead_code)]
    pub async fn tcp_send(
        &self,
        task_id: u64,
        socket_id: SocketId,
        bytes: Vec<u8>,
    ) -> Result<usize> {
        let sockets = self.tcp_sockets.read().await;
        let session = sockets.get(&socket_id).context("unknown tcp socket id")?;
        if session.task_id != task_id {
            bail!("task {task_id} cannot use tcp socket {socket_id}")
        }
        if bytes.len() > session.max_message_bytes {
            bail!("tcp payload exceeded {} bytes", session.max_message_bytes)
        }
        let mut stream = session.stream.lock().await;
        timeout(
            Duration::from_millis(session.io_timeout_ms),
            stream.write_all(&bytes),
        )
        .await??;
        Ok(bytes.len())
    }

    #[allow(dead_code)]
    pub async fn tcp_recv(
        &self,
        task_id: u64,
        socket_id: SocketId,
        max_len: usize,
    ) -> Result<Vec<u8>> {
        let sockets = self.tcp_sockets.read().await;
        let session = sockets.get(&socket_id).context("unknown tcp socket id")?;
        if session.task_id != task_id {
            bail!("task {task_id} cannot use tcp socket {socket_id}")
        }
        let read_len = max_len.min(session.max_message_bytes);
        let mut stream = session.stream.lock().await;
        let mut buffer = vec![0u8; read_len];
        let n = timeout(
            Duration::from_millis(session.io_timeout_ms),
            stream.read(&mut buffer),
        )
        .await??;
        buffer.truncate(n);
        Ok(buffer)
    }

    async fn next_socket_id(&self) -> SocketId {
        let mut guard = self.next_socket_id.lock().await;
        let current = *guard;
        *guard += 1;
        current
    }

    async fn policy_for(&self, task_id: u64) -> SocketPolicy {
        self.policies
            .read()
            .await
            .get(&task_id)
            .cloned()
            .unwrap_or_default()
    }

    async fn record_error(&self, task_id: u64, message: String) {
        self.last_errors.write().await.insert(task_id, message);
    }

    async fn clear_error(&self, task_id: u64) {
        self.last_errors.write().await.remove(&task_id);
    }

    async fn enforce(&self, task_id: u64, host: &str, capability: NetworkCapability) -> Result<()> {
        let policy = self.policy_for(task_id).await;

        if !policy.allow_remote {
            bail!("network disabled for task {task_id}")
        }
        match capability {
            NetworkCapability::Http if !policy.allow_http => {
                bail!("HTTP disabled for task {task_id}")
            }
            NetworkCapability::WebSocket if !policy.allow_websocket => {
                bail!("WebSocket disabled for task {task_id}")
            }
            NetworkCapability::Tcp if !policy.allow_tcp => bail!("TCP disabled for task {task_id}"),
            _ => {}
        }
        if !policy.dns_allowlist.is_empty()
            && !policy
                .dns_allowlist
                .iter()
                .any(|entry| entry.eq_ignore_ascii_case(host))
        {
            bail!("dns host {host} not permitted")
        }
        if !policy.denied_hosts.is_empty()
            && policy
                .denied_hosts
                .iter()
                .any(|entry| entry.eq_ignore_ascii_case(host))
        {
            bail!("host {host} explicitly denied")
        }
        if !policy.allowed_hosts.is_empty()
            && !policy
                .allowed_hosts
                .iter()
                .any(|entry| entry.eq_ignore_ascii_case(host))
        {
            bail!("host {host} not permitted")
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network_fixture() -> NetworkSubsystem {
        NetworkSubsystem::new(Arc::new(HostBridge::detect()))
    }

    #[tokio::test]
    async fn http_request_denied_when_remote_network_disabled() {
        let network = network_fixture();
        let task_id = 21;
        network.set_policy(task_id, SocketPolicy::default()).await;
        let request = HttpRequest {
            method: "GET".to_string(),
            url: Url::parse("https://example.com").expect("url should parse"),
            headers: BTreeMap::new(),
            body: Vec::new(),
        };
        let err = network
            .http_request(task_id, request)
            .await
            .expect_err("policy should block remote HTTP");
        assert!(
            err.to_string().contains("network disabled"),
            "expected remote network denial, got {err:?}"
        );
    }

    #[tokio::test]
    async fn tcp_connect_respects_allowlist_policy() {
        let network = network_fixture();
        let task_id = 22;
        network
            .set_policy(
                task_id,
                SocketPolicy {
                    allow_remote: true,
                    allow_http: false,
                    allow_websocket: false,
                    allow_tcp: true,
                    allowed_hosts: vec!["example.com".to_string()],
                    denied_hosts: Vec::new(),
                    dns_allowlist: Vec::new(),
                    connect_timeout_ms: 10,
                    io_timeout_ms: 10,
                    max_response_bytes: 1024,
                    max_message_bytes: 256,
                },
            )
            .await;
        let err = network
            .tcp_connect(task_id, "127.0.0.1", 9)
            .await
            .expect_err("host outside allowlist should be denied");
        assert!(
            err.to_string().contains("not permitted"),
            "expected allowlist denial, got {err:?}"
        );
    }

    #[tokio::test]
    async fn tcp_connect_timeout_or_connect_error_is_reported() {
        let network = network_fixture();
        let task_id = 23;
        network
            .set_policy(
                task_id,
                SocketPolicy {
                    allow_remote: true,
                    allow_http: false,
                    allow_websocket: false,
                    allow_tcp: true,
                    allowed_hosts: vec!["203.0.113.1".to_string()],
                    denied_hosts: Vec::new(),
                    dns_allowlist: vec!["203.0.113.1".to_string()],
                    connect_timeout_ms: 1,
                    io_timeout_ms: 5,
                    max_response_bytes: 1024,
                    max_message_bytes: 256,
                },
            )
            .await;
        let err = network
            .tcp_connect(task_id, "203.0.113.1", 65000)
            .await
            .expect_err("unreachable fixture endpoint should fail fast");
        assert!(
            !err.to_string().is_empty(),
            "network error should provide a message"
        );
    }

    #[tokio::test]
    async fn network_subsystem_remembers_last_http_error_message() {
        let network = network_fixture();
        let task_id = 24;
        network
            .set_policy(
                task_id,
                SocketPolicy {
                    allow_remote: true,
                    allow_http: true,
                    allow_websocket: false,
                    allow_tcp: false,
                    allowed_hosts: vec!["203.0.113.1".to_string()],
                    denied_hosts: Vec::new(),
                    dns_allowlist: vec!["203.0.113.1".to_string()],
                    connect_timeout_ms: 1,
                    io_timeout_ms: 5,
                    max_response_bytes: 1024,
                    max_message_bytes: 256,
                },
            )
            .await;
        let _ = network
            .http_request(
                task_id,
                HttpRequest {
                    method: "GET".to_string(),
                    url: Url::parse("http://203.0.113.1").expect("url should parse"),
                    headers: BTreeMap::new(),
                    body: Vec::new(),
                },
            )
            .await;

        let message = network
            .last_error(task_id)
            .await
            .expect("failed request should record an error message");
        assert!(
            message.contains("HTTP"),
            "expected recorded error message, got {message:?}"
        );
    }
}
