# WasmOS ABI Specification (v1)

Version: `wasmos.v1`
Shared guest constants/types crate: `guest_abi` (`wasmos_guest_abi`).

## Transport and status model

- ABI imports are provided from module: `wasmos`.
- Status return value is a signed 32-bit integer (`i32`) mapped to `OsErrorCode`.
- Complex request/response payloads are JSON-encoded structs from `wasmos_guest_abi`.
- Buffer-returning calls use caller-provided output buffers + `bytes_written_ptr`.

## `OsErrorCode` numeric mapping

| Code | Name |
|---:|---|
| 0 | Ok |
| 1 | InvalidArgument |
| 2 | InvalidUtf8 |
| 3 | MemoryOutOfBounds |
| 4 | BufferTooSmall |
| 5 | Serialization |
| 6 | NotFound |
| 7 | AlreadyExists |
| 8 | NotSupported |
| 9 | PermissionDenied |
| 10 | Timeout |
| 11 | NetworkUnavailable |
| 12 | BadHandle |
| 13 | Conflict |
| 255 | Internal |

## Syscalls

### Scheduling

- `yield_now() -> i32`
- `sleep_ms(duration_ms: i64) -> i32`
- `wait_event(channel_ptr: i32, channel_len: i32) -> i32`

### VFS

- `vfs_open(req_ptr, req_len, fd_ptr) -> i32` (`VfsOpenRequest`)
- `vfs_close(fd: u64) -> i32`
- `vfs_fd_read(req_ptr, req_len, out_ptr, out_len, bytes_written_ptr) -> i32` (`VfsReadRequest`)
- `vfs_fd_write(req_ptr, req_len, bytes_written_ptr) -> i32` (`VfsWriteRequest`)
- `vfs_seek(req_ptr, req_len, pos_ptr) -> i32` (`VfsSeekRequest`)
- `vfs_list_dir(req_ptr, req_len, out_ptr, out_len, bytes_written_ptr) -> i32` (`VfsPathRequest` => `Vec<VfsDirEntry>`)
- `vfs_mkdir(req_ptr, req_len) -> i32` (`VfsPathRequest`)
- `vfs_delete(req_ptr, req_len) -> i32` (`VfsPathRequest`)

### Networking

- `net_http(req_ptr, req_len, out_ptr, out_len, bytes_written_ptr) -> i32` (`HttpRequest` => `HttpResponse`)
- `net_ws_open(req_ptr, req_len, socket_id_ptr) -> i32` (`NetWebSocketOpenRequest`)
- `net_ws_send_text(req_ptr, req_len) -> i32` (`NetWebSocketSendRequest`)
- `net_tcp_connect(req_ptr, req_len, socket_id_ptr) -> i32` (`NetTcpConnectRequest`)

### GUI

- `gui_open_window(desc_ptr, desc_len, window_id_ptr) -> i32` (`WindowDescriptor`)
- `gui_draw(req_ptr, req_len) -> i32` (`GuiDrawRequest`)
- `gui_poll_events(req_ptr, req_len, out_ptr, out_len, bytes_written_ptr) -> i32` (`GuiPollRequest` => `Vec<GuiEvent>`)

## Request/response struct source of truth

Use `wasmos_guest_abi` for all shared wire structs and constants:

- Open flags (`OPEN_*`)
- VFS structs (`VfsOpenRequest`, `VfsReadRequest`, `VfsWriteRequest`, `VfsSeekRequest`, `VfsPathRequest`, `VfsDirEntry`)
- Network structs (`HttpRequest`, `HttpResponse`, `NetWebSocketOpenRequest`, `NetWebSocketSendRequest`, `NetTcpConnectRequest`)
- GUI structs (`WindowDescriptor`, `DrawCommand`, `GuiDrawRequest`, `GuiPollRequest`, `GuiEvent`)
- Error constants (`OsErrorCode`)

