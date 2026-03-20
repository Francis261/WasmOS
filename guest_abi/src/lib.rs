use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use url::Url;

pub const ABI_VERSION: &str = "wasmos.v1";

pub mod syscall {
    pub const YIELD_NOW: &str = "yield_now";
    pub const SLEEP_MS: &str = "sleep_ms";
    pub const WAIT_EVENT: &str = "wait_event";

    pub const VFS_OPEN: &str = "vfs_open";
    pub const VFS_CLOSE: &str = "vfs_close";
    pub const VFS_FD_READ: &str = "vfs_fd_read";
    pub const VFS_FD_WRITE: &str = "vfs_fd_write";
    pub const VFS_SEEK: &str = "vfs_seek";
    pub const VFS_LIST_DIR: &str = "vfs_list_dir";
    pub const VFS_MKDIR: &str = "vfs_mkdir";
    pub const VFS_DELETE: &str = "vfs_delete";

    pub const NET_HTTP: &str = "net_http";
    pub const NET_WS_OPEN: &str = "net_ws_open";
    pub const NET_WS_SEND_TEXT: &str = "net_ws_send_text";
    pub const NET_TCP_CONNECT: &str = "net_tcp_connect";

    pub const GUI_OPEN_WINDOW: &str = "gui_open_window";
    pub const GUI_DRAW: &str = "gui_draw";
    pub const GUI_POLL_EVENTS: &str = "gui_poll_events";
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[repr(i32)]
pub enum OsErrorCode {
    Ok = 0,
    InvalidArgument = 1,
    InvalidUtf8 = 2,
    MemoryOutOfBounds = 3,
    BufferTooSmall = 4,
    Serialization = 5,
    NotFound = 6,
    AlreadyExists = 7,
    NotSupported = 8,
    PermissionDenied = 9,
    Timeout = 10,
    NetworkUnavailable = 11,
    BadHandle = 12,
    Conflict = 13,
    Internal = 255,
}

impl OsErrorCode {
    pub fn from_i32(value: i32) -> Option<Self> {
        Some(match value {
            0 => Self::Ok,
            1 => Self::InvalidArgument,
            2 => Self::InvalidUtf8,
            3 => Self::MemoryOutOfBounds,
            4 => Self::BufferTooSmall,
            5 => Self::Serialization,
            6 => Self::NotFound,
            7 => Self::AlreadyExists,
            8 => Self::NotSupported,
            9 => Self::PermissionDenied,
            10 => Self::Timeout,
            11 => Self::NetworkUnavailable,
            12 => Self::BadHandle,
            13 => Self::Conflict,
            255 => Self::Internal,
            _ => return None,
        })
    }
}

pub const OPEN_READ: u32 = 1 << 0;
pub const OPEN_WRITE: u32 = 1 << 1;
pub const OPEN_CREATE: u32 = 1 << 2;
pub const OPEN_TRUNCATE: u32 = 1 << 3;
pub const OPEN_APPEND: u32 = 1 << 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsOpenRequest {
    pub path: String,
    pub flags: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsReadRequest {
    pub fd: u64,
    pub len: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsWriteRequest {
    pub fd: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SeekWhence {
    Start,
    Current,
    End,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsSeekRequest {
    pub fd: u64,
    pub offset: i64,
    pub whence: SeekWhence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsPathRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsDirEntry {
    pub path: String,
    pub kind: NodeKind,
    pub len: usize,
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
pub struct WindowDescriptor {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DrawCommand {
    Clear { rgba: [u8; 4] },
    Pixel { x: u32, y: u32, rgba: [u8; 4] },
    Text {
        x: u32,
        y: u32,
        text: String,
        rgba: [u8; 4],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiDrawRequest {
    pub window_id: u64,
    pub commands: Vec<DrawCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiPollRequest {
    pub window_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GuiEvent {
    KeyDown { key_code: u32 },
    KeyUp { key_code: u32 },
    MouseMove { x: i32, y: i32 },
    MouseClick { x: i32, y: i32, button: u8 },
    CloseRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetWebSocketOpenRequest {
    pub url: Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetWebSocketSendRequest {
    pub socket_id: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetTcpConnectRequest {
    pub host: String,
    pub port: u16,
}
