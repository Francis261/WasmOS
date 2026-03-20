use crate::gui::{DrawCommand, GuiEvent, WindowDescriptor};
use crate::network::{HttpRequest, HttpResponse};
use crate::os_error::{OsError, OsErrorCode};
use crate::runtime::RuntimeContext;
use crate::vfs::{SeekWhence, VfsDirEntry, VfsError};
use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::str;
use wasmtime::{Caller, Linker, Memory};

const MAX_GUEST_BUFFER: usize = 64 * 1024;

pub struct SystemCallRegistry;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VfsOpenRequest {
    path: String,
    flags: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VfsReadRequest {
    fd: u64,
    len: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VfsWriteRequest {
    fd: u64,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VfsSeekRequest {
    fd: u64,
    offset: i64,
    whence: SeekWhence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VfsPathRequest {
    path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NetWebSocketOpenRequest {
    url: url::Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NetWebSocketSendRequest {
    socket_id: u64,
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NetTcpConnectRequest {
    host: String,
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GuiDrawRequest {
    window_id: u64,
    commands: Vec<DrawCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GuiPollRequest {
    window_id: u64,
}

impl SystemCallRegistry {
    pub fn link(linker: &mut Linker<RuntimeContext>) -> Result<()> {
        linker.func_wrap_async(
            "wasmos",
            "yield_now",
            |caller: Caller<'_, RuntimeContext>, ()| {
                Box::new(async move {
                    caller.data().control.lock().await.request_yield();
                    Ok(AbiStatus::Ok.as_i32())
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "sleep_ms",
            |caller: Caller<'_, RuntimeContext>, (duration_ms,): (i64,)| {
                Box::new(async move {
                    if duration_ms < 0 {
                        return Ok(AbiStatus::InvalidArgument.as_i32());
                    }
                    caller
                        .data()
                        .control
                        .lock()
                        .await
                        .request_sleep(duration_ms as u64);
                    Ok(AbiStatus::Ok.as_i32())
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "wait_event",
            |mut caller: Caller<'_, RuntimeContext>, (channel_ptr, channel_len): (i32, i32)| {
                let channel = match read_guest_string(&mut caller, channel_ptr, channel_len) {
                    Ok(channel) => channel,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                Box::new(async move {
                    caller.data().control.lock().await.request_io_wait(channel);
                    Ok(AbiStatus::Ok.as_i32())
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "vfs_read",
            |mut caller: Caller<'_, RuntimeContext>,
             (path_ptr, path_len, buf_ptr, buf_len, bytes_written_ptr): (
                i32,
                i32,
                i32,
                i32,
                i32,
            )| {
                let path = match read_guest_string(&mut caller, path_ptr, path_len) {
                    Ok(path) => path,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                let vfs = caller.data().vfs.clone();
                Box::new(async move {
                    let result = match vfs.read().await.read_file(path) {
                        Ok(data) => {
                            let max_len = match clamp_guest_len(buf_len) {
                                Ok(value) => value,
                                Err(error) => return Ok(error.status().as_i32()),
                            };
                            let bytes_to_write = data.len().min(max_len);
                            if let Err(error) =
                                write_guest_bytes(&mut caller, buf_ptr, &data[..bytes_to_write])
                            {
                                return Ok(error.status().as_i32());
                            }
                            if let Err(error) = write_guest_u32(
                                &mut caller,
                                bytes_written_ptr,
                                bytes_to_write as u32,
                            ) {
                                return Ok(error.status().as_i32());
                            }
                            if bytes_to_write < data.len() {
                                Err(AbiError::BufferTooSmall {
                                    required: data.len(),
                                    available: max_len,
                                })
                            } else {
                                Ok(())
                            }
                        }
                        Err(error) => Err(AbiError::from(error)),
                    };
                    Ok(status_from_result(result))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "vfs_write",
            |mut caller: Caller<'_, RuntimeContext>,
             (path_ptr, path_len, buf_ptr, buf_len): (i32, i32, i32, i32)| {
                let path = match read_guest_string(&mut caller, path_ptr, path_len) {
                    Ok(path) => path,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                let content = match read_guest_bytes(&mut caller, buf_ptr, buf_len) {
                    Ok(content) => content,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                let vfs = caller.data().vfs.clone();
                Box::new(async move {
                    let result = vfs
                        .write()
                        .await
                        .write_file(path, content)
                        .map_err(AbiError::from);
                    Ok(status_from_result(result))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "vfs_open",
            |mut caller: Caller<'_, RuntimeContext>,
             (req_ptr, req_len, fd_ptr): (i32, i32, i32)| {
                let request: VfsOpenRequest = match read_guest_struct(&mut caller, req_ptr, req_len)
                {
                    Ok(request) => request,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                let task_id = caller.data().task_id;
                let vfs = caller.data().vfs.clone();
                Box::new(async move {
                    let fd =
                        match vfs
                            .write()
                            .await
                            .open_for_task(task_id, request.path, request.flags)
                        {
                            Ok(fd) => fd,
                            Err(error) => return Ok(AbiError::from(error).status().as_i32()),
                        };
                    Ok(status_from_result(write_guest_u64(&mut caller, fd_ptr, fd)))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "vfs_close",
            |caller: Caller<'_, RuntimeContext>, (fd,): (u64,)| {
                let task_id = caller.data().task_id;
                let vfs = caller.data().vfs.clone();
                Box::new(async move {
                    let result = vfs
                        .write()
                        .await
                        .close_for_task(task_id, fd)
                        .map_err(AbiError::from);
                    Ok(status_from_result(result))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "vfs_fd_read",
            |mut caller: Caller<'_, RuntimeContext>,
             (req_ptr, req_len, out_ptr, out_len, bytes_written_ptr): (i32, i32, i32, i32, i32)| {
                let request: VfsReadRequest = match read_guest_struct(&mut caller, req_ptr, req_len) {
                    Ok(request) => request,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                let task_id = caller.data().task_id;
                let vfs = caller.data().vfs.clone();
                Box::new(async move {
                    let payload = match vfs
                        .write()
                        .await
                        .read_for_task(task_id, request.fd, request.len as usize)
                    {
                        Ok(data) => data,
                        Err(error) => return Ok(AbiError::from(error).status().as_i32()),
                    };
                    let max_len = match clamp_guest_len(out_len) {
                        Ok(value) => value,
                        Err(error) => return Ok(error.status().as_i32()),
                    };
                    let bytes_to_write = payload.len().min(max_len);
                    if let Err(error) = write_guest_bytes(&mut caller, out_ptr, &payload[..bytes_to_write]) {
                        return Ok(error.status().as_i32());
                    }
                    if let Err(error) = write_guest_u32(&mut caller, bytes_written_ptr, bytes_to_write as u32) {
                        return Ok(error.status().as_i32());
                    }
                    if payload.len() > max_len {
                        return Ok(AbiStatus::BufferTooSmall.as_i32());
                    }
                    Ok(AbiStatus::Ok.as_i32())
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "vfs_fd_write",
            |mut caller: Caller<'_, RuntimeContext>,
             (req_ptr, req_len, bytes_written_ptr): (i32, i32, i32)| {
                let request: VfsWriteRequest =
                    match read_guest_struct(&mut caller, req_ptr, req_len) {
                        Ok(request) => request,
                        Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                    };
                let task_id = caller.data().task_id;
                let vfs = caller.data().vfs.clone();
                Box::new(async move {
                    let written =
                        match vfs
                            .write()
                            .await
                            .write_for_task(task_id, request.fd, &request.data)
                        {
                            Ok(bytes) => bytes,
                            Err(error) => return Ok(AbiError::from(error).status().as_i32()),
                        };
                    Ok(status_from_result(write_guest_u32(
                        &mut caller,
                        bytes_written_ptr,
                        written as u32,
                    )))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "vfs_seek",
            |mut caller: Caller<'_, RuntimeContext>,
             (req_ptr, req_len, pos_ptr): (i32, i32, i32)| {
                let request: VfsSeekRequest = match read_guest_struct(&mut caller, req_ptr, req_len)
                {
                    Ok(request) => request,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                let task_id = caller.data().task_id;
                let vfs = caller.data().vfs.clone();
                Box::new(async move {
                    let position = match vfs.write().await.seek_for_task(
                        task_id,
                        request.fd,
                        request.offset,
                        request.whence,
                    ) {
                        Ok(position) => position,
                        Err(error) => return Ok(AbiError::from(error).status().as_i32()),
                    };
                    Ok(status_from_result(write_guest_u64(
                        &mut caller,
                        pos_ptr,
                        position,
                    )))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "vfs_list_dir",
            |mut caller: Caller<'_, RuntimeContext>,
             (req_ptr, req_len, out_ptr, out_len, bytes_written_ptr): (i32, i32, i32, i32, i32)| {
                let request: VfsPathRequest = match read_guest_struct(&mut caller, req_ptr, req_len) {
                    Ok(request) => request,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                let task_id = caller.data().task_id;
                let vfs = caller.data().vfs.clone();
                Box::new(async move {
                    let entries: Vec<VfsDirEntry> = match vfs.read().await.list_dir_for_task(task_id, request.path) {
                        Ok(entries) => entries,
                        Err(error) => return Ok(AbiError::from(error).status().as_i32()),
                    };
                    Ok(status_from_result(write_serialized_response(
                        &mut caller,
                        out_ptr,
                        out_len,
                        bytes_written_ptr,
                        &entries,
                    )))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "vfs_mkdir",
            |mut caller: Caller<'_, RuntimeContext>, (req_ptr, req_len): (i32, i32)| {
                let request: VfsPathRequest = match read_guest_struct(&mut caller, req_ptr, req_len)
                {
                    Ok(request) => request,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                let task_id = caller.data().task_id;
                let vfs = caller.data().vfs.clone();
                Box::new(async move {
                    let result = vfs
                        .write()
                        .await
                        .create_dir_for_task(task_id, request.path)
                        .map_err(AbiError::from);
                    Ok(status_from_result(result))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "vfs_delete",
            |mut caller: Caller<'_, RuntimeContext>, (req_ptr, req_len): (i32, i32)| {
                let request: VfsPathRequest = match read_guest_struct(&mut caller, req_ptr, req_len)
                {
                    Ok(request) => request,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                let task_id = caller.data().task_id;
                let vfs = caller.data().vfs.clone();
                Box::new(async move {
                    let result = vfs
                        .write()
                        .await
                        .delete_for_task(task_id, request.path)
                        .map_err(AbiError::from);
                    Ok(status_from_result(result))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "net_http",
            |mut caller: Caller<'_, RuntimeContext>,
             (req_ptr, req_len, resp_ptr, resp_len, bytes_written_ptr): (
                i32,
                i32,
                i32,
                i32,
                i32,
            )| {
                let request: HttpRequest = match read_guest_struct(&mut caller, req_ptr, req_len) {
                    Ok(request) => request,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                let task_id = caller.data().task_id;
                let network = caller.data().network.clone();
                Box::new(async move {
                    let response = match network.http_request(task_id, request).await {
                        Ok(response) => response,
                        Err(error) => return Ok(AbiError::from(error).status().as_i32()),
                    };
                    Ok(status_from_result(write_serialized_response(
                        &mut caller,
                        resp_ptr,
                        resp_len,
                        bytes_written_ptr,
                        &response,
                    )))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "net_ws_open",
            |mut caller: Caller<'_, RuntimeContext>,
             (req_ptr, req_len, socket_id_ptr): (i32, i32, i32)| {
                let request: NetWebSocketOpenRequest =
                    match read_guest_struct(&mut caller, req_ptr, req_len) {
                        Ok(request) => request,
                        Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                    };
                let task_id = caller.data().task_id;
                let network = caller.data().network.clone();
                Box::new(async move {
                    let socket_id = match network.websocket_open(task_id, request.url).await {
                        Ok(socket_id) => socket_id,
                        Err(error) => return Ok(AbiError::from(error).status().as_i32()),
                    };
                    Ok(status_from_result(write_guest_u64(
                        &mut caller,
                        socket_id_ptr,
                        socket_id,
                    )))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "net_ws_send_text",
            |mut caller: Caller<'_, RuntimeContext>, (req_ptr, req_len): (i32, i32)| {
                let request: NetWebSocketSendRequest =
                    match read_guest_struct(&mut caller, req_ptr, req_len) {
                        Ok(request) => request,
                        Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                    };
                let task_id = caller.data().task_id;
                let network = caller.data().network.clone();
                Box::new(async move {
                    let result = network
                        .websocket_send_text(task_id, request.socket_id, request.text)
                        .await
                        .map_err(AbiError::from);
                    Ok(status_from_result(result))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "net_tcp_connect",
            |mut caller: Caller<'_, RuntimeContext>,
             (req_ptr, req_len, socket_id_ptr): (i32, i32, i32)| {
                let request: NetTcpConnectRequest =
                    match read_guest_struct(&mut caller, req_ptr, req_len) {
                        Ok(request) => request,
                        Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                    };
                let task_id = caller.data().task_id;
                let network = caller.data().network.clone();
                Box::new(async move {
                    let socket_id = match network
                        .tcp_connect(task_id, &request.host, request.port)
                        .await
                    {
                        Ok(socket_id) => socket_id,
                        Err(error) => return Ok(AbiError::from(error).status().as_i32()),
                    };
                    Ok(status_from_result(write_guest_u64(
                        &mut caller,
                        socket_id_ptr,
                        socket_id,
                    )))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "gui_open_window",
            |mut caller: Caller<'_, RuntimeContext>,
             (desc_ptr, desc_len, window_id_ptr): (i32, i32, i32)| {
                let descriptor: WindowDescriptor =
                    match read_guest_struct(&mut caller, desc_ptr, desc_len) {
                        Ok(descriptor) => descriptor,
                        Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                    };
                let task_id = caller.data().task_id;
                let gui = caller.data().gui.clone();
                Box::new(async move {
                    let window_id = gui.create_window(task_id, descriptor).await;
                    Ok(status_from_result(write_guest_u64(
                        &mut caller,
                        window_id_ptr,
                        window_id,
                    )))
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "gui_draw",
            |mut caller: Caller<'_, RuntimeContext>, (req_ptr, req_len): (i32, i32)| {
                let request: GuiDrawRequest = match read_guest_struct(&mut caller, req_ptr, req_len)
                {
                    Ok(request) => request,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                let task_id = caller.data().task_id;
                let gui = caller.data().gui.clone();
                Box::new(async move {
                    gui.draw(task_id, request.window_id, request.commands).await;
                    Ok(AbiStatus::Ok.as_i32())
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "gui_poll_events",
            |mut caller: Caller<'_, RuntimeContext>,
             (req_ptr, req_len, out_ptr, out_len, bytes_written_ptr): (i32, i32, i32, i32, i32)| {
                let request: GuiPollRequest = match read_guest_struct(&mut caller, req_ptr, req_len) {
                    Ok(request) => request,
                    Err(error) => return Box::new(async move { Ok(error.status().as_i32()) }),
                };
                let task_id = caller.data().task_id;
                let gui = caller.data().gui.clone();
                Box::new(async move {
                    let events = gui.poll_events(task_id, request.window_id).await;
                    Ok(status_from_result(write_serialized_response(
                        &mut caller,
                        out_ptr,
                        out_len,
                        bytes_written_ptr,
                        &events,
                    )))
                })
            },
        )?;

        Ok(())
    }
}

pub type AbiStatus = OsErrorCode;

#[derive(Debug)]
#[allow(dead_code)]
pub enum AbiError {
    Structured(OsError),
    InvalidUtf8(std::str::Utf8Error),
    MemoryOutOfBounds { ptr: usize, len: usize },
    BufferTooSmall { required: usize, available: usize },
}

impl AbiError {
    fn status(&self) -> AbiStatus {
        match self {
            Self::Structured(error) => error.code,
            Self::InvalidUtf8(_) => AbiStatus::InvalidUtf8,
            Self::MemoryOutOfBounds { .. } => AbiStatus::MemoryOutOfBounds,
            Self::BufferTooSmall { .. } => AbiStatus::BufferTooSmall,
        }
    }
}

impl From<VfsError> for AbiError {
    fn from(value: VfsError) -> Self {
        Self::Structured(OsError::from(value))
    }
}

impl From<anyhow::Error> for AbiError {
    fn from(value: anyhow::Error) -> Self {
        Self::Structured(OsError::from(value))
    }
}

fn status_from_result(result: Result<(), AbiError>) -> i32 {
    match result {
        Ok(()) => AbiStatus::Ok.as_i32(),
        Err(error) => error.status().as_i32(),
    }
}

pub fn read_guest_bytes(
    caller: &mut Caller<'_, RuntimeContext>,
    ptr: i32,
    len: i32,
) -> Result<Vec<u8>, AbiError> {
    let ptr = clamp_guest_offset(ptr)?;
    let len = clamp_guest_len(len)?;
    let memory = guest_memory(caller)?;
    ensure_memory_range(caller, &memory, ptr, len)?;

    let mut buffer = vec![0_u8; len];
    memory
        .read(caller, ptr, &mut buffer)
        .map_err(|_| AbiError::MemoryOutOfBounds { ptr, len })?;
    Ok(buffer)
}

pub fn read_guest_string(
    caller: &mut Caller<'_, RuntimeContext>,
    ptr: i32,
    len: i32,
) -> Result<String, AbiError> {
    let bytes = read_guest_bytes(caller, ptr, len)?;
    let text = str::from_utf8(&bytes).map_err(AbiError::InvalidUtf8)?;
    Ok(text.to_owned())
}

pub fn read_guest_struct<T: DeserializeOwned>(
    caller: &mut Caller<'_, RuntimeContext>,
    ptr: i32,
    len: i32,
) -> Result<T, AbiError> {
    let bytes = read_guest_bytes(caller, ptr, len)?;
    serde_json::from_slice(&bytes).map_err(|error| {
        AbiError::Structured(OsError::new(OsErrorCode::Serialization, error.to_string()))
    })
}

pub fn write_guest_bytes(
    caller: &mut Caller<'_, RuntimeContext>,
    ptr: i32,
    data: &[u8],
) -> Result<(), AbiError> {
    let ptr = clamp_guest_offset(ptr)?;
    let memory = guest_memory(caller)?;
    ensure_memory_range(caller, &memory, ptr, data.len())?;
    memory
        .write(caller, ptr, data)
        .map_err(|_| AbiError::MemoryOutOfBounds {
            ptr,
            len: data.len(),
        })
}

pub fn write_guest_u32(
    caller: &mut Caller<'_, RuntimeContext>,
    ptr: i32,
    value: u32,
) -> Result<(), AbiError> {
    write_guest_bytes(caller, ptr, &value.to_le_bytes())
}

pub fn write_guest_u64(
    caller: &mut Caller<'_, RuntimeContext>,
    ptr: i32,
    value: u64,
) -> Result<(), AbiError> {
    write_guest_bytes(caller, ptr, &value.to_le_bytes())
}

fn write_serialized_response<T: Serialize>(
    caller: &mut Caller<'_, RuntimeContext>,
    ptr: i32,
    len: i32,
    bytes_written_ptr: i32,
    value: &T,
) -> Result<(), AbiError> {
    let encoded = serde_json::to_vec(value).map_err(|error| {
        AbiError::Structured(OsError::new(OsErrorCode::Serialization, error.to_string()))
    })?;
    let capacity = clamp_guest_len(len)?;
    let bytes_to_write = encoded.len().min(capacity);
    write_guest_bytes(caller, ptr, &encoded[..bytes_to_write])?;
    write_guest_u32(caller, bytes_written_ptr, bytes_to_write as u32)?;
    if bytes_to_write < encoded.len() {
        return Err(AbiError::BufferTooSmall {
            required: encoded.len(),
            available: capacity,
        });
    }
    Ok(())
}

fn guest_memory(caller: &mut Caller<'_, RuntimeContext>) -> Result<Memory, AbiError> {
    caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
        .ok_or(AbiError::Structured(OsError::new(
            OsErrorCode::NotSupported,
            "guest module must export linear memory named `memory`",
        )))
}

fn ensure_memory_range(
    caller: &mut Caller<'_, RuntimeContext>,
    memory: &Memory,
    ptr: usize,
    len: usize,
) -> Result<(), AbiError> {
    let end = ptr
        .checked_add(len)
        .ok_or(AbiError::MemoryOutOfBounds { ptr, len })?;
    if end > memory.data_size(caller) {
        return Err(AbiError::MemoryOutOfBounds { ptr, len });
    }
    Ok(())
}

fn clamp_guest_offset(ptr: i32) -> Result<usize, AbiError> {
    usize::try_from(ptr).map_err(|_| {
        AbiError::Structured(OsError::invalid_argument("pointer must be non-negative"))
    })
}

fn clamp_guest_len(len: i32) -> Result<usize, AbiError> {
    let len = usize::try_from(len).map_err(|_| {
        AbiError::Structured(OsError::invalid_argument("length must be non-negative"))
    })?;
    if len > MAX_GUEST_BUFFER {
        return Err(AbiError::Structured(OsError::invalid_argument(
            "length exceeds syscall ABI limit",
        )));
    }
    Ok(len)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct GuestApi;

#[allow(dead_code)]
impl GuestApi {
    pub fn yield_now() {}
    pub fn file_read(_path: &str) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }
    pub fn file_write(_path: &str, _bytes: &[u8]) -> Result<()> {
        Ok(())
    }
    pub fn http_request(_request: HttpRequest) -> Result<HttpResponse> {
        Ok(HttpResponse {
            status: 501,
            headers: Default::default(),
            body: Vec::new(),
        })
    }
    pub fn create_window(_descriptor: WindowDescriptor) -> Result<u64> {
        Ok(0)
    }
    pub fn draw(_window_id: u64, _commands: &[DrawCommand]) -> Result<()> {
        Ok(())
    }
    pub fn poll_events(_window_id: u64) -> Result<Vec<GuiEvent>> {
        Ok(Vec::new())
    }
}
