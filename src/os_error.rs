use crate::vfs::VfsError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsError {
    pub code: OsErrorCode,
    pub message: String,
}

impl OsError {
    pub fn new(code: OsErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(OsErrorCode::InvalidArgument, message)
    }

    #[allow(dead_code)]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(OsErrorCode::Internal, message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(OsErrorCode::Timeout, message)
    }
}

impl From<VfsError> for OsError {
    fn from(value: VfsError) -> Self {
        match value {
            VfsError::NotFound(path) => Self::new(OsErrorCode::NotFound, path),
            VfsError::AlreadyExists(path) => Self::new(OsErrorCode::AlreadyExists, path),
            VfsError::NotAFile(path) => {
                Self::new(OsErrorCode::InvalidArgument, format!("not a file: {path}"))
            }
            VfsError::NotADirectory(path) => Self::new(
                OsErrorCode::InvalidArgument,
                format!("not a directory: {path}"),
            ),
            VfsError::PermissionDenied(message) => {
                Self::new(OsErrorCode::PermissionDenied, message)
            }
            VfsError::InvalidFd(fd) => {
                Self::new(OsErrorCode::BadHandle, format!("invalid fd {fd}"))
            }
            VfsError::InvalidOpenFlags => {
                Self::new(OsErrorCode::InvalidArgument, "invalid open flags")
            }
            VfsError::InvalidSeek => Self::new(OsErrorCode::InvalidArgument, "invalid seek"),
            VfsError::Snapshot(message) => Self::new(OsErrorCode::Internal, message),
        }
    }
}

impl From<anyhow::Error> for OsError {
    fn from(value: anyhow::Error) -> Self {
        let message = value.to_string();
        if message.contains("timed out") || message.contains("deadline has elapsed") {
            return Self::timeout(message);
        }
        if message.contains("disabled for task") || message.contains("not permitted") {
            return Self::new(OsErrorCode::PermissionDenied, message);
        }
        if message.contains("not implemented") {
            return Self::new(OsErrorCode::NotSupported, message);
        }
        if message.contains("dns")
            || message.contains("HTTP request failed")
            || message.contains("HTTP body read failed")
            || message.contains("tcp connect failed")
            || message.contains("websocket connect failed")
            || message.contains("certificate")
            || message.contains("tls")
            || message.contains("connection")
        {
            return Self::new(OsErrorCode::NetworkUnavailable, message);
        }
        Self::new(OsErrorCode::Internal, message)
    }
}
