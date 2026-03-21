# WasmOS module design

## Layer boundaries

```text
Shell -> Scheduler -> RuntimeHost -> { VFS, NetManager, GuiBroker } -> Host OS
```

## Core data types

### Shell
- `Shell`: parses commands, performs lightweight validation, and delegates process creation.
- `ShellResponse`: normalized terminal output contract for the web desktop.

### Scheduler
- `SpawnRequest`: app ID, Wasm module path, and argument vector.
- `TaskRecord`: task metadata stored in the scheduler table.
- `TaskState`: `ready`, `running`, `yielded`, `exited`, `faulted`.

### RuntimeHost
- Initializes Wasmtime with async support, component model support, and fuel accounting.
- Validates that each program is a `.wasm` module.
- Materializes capability views before execution.

### VirtualFileSystem
- Uses an in-memory `BTreeMap<String, Vec<u8>>` for file blobs.
- Keeps directory membership in a `BTreeSet<String>`.
- Restricts access to `/apps`, `/data`, and `/system`.

### NetManager
- Stores per-app `NetPolicy` allowlists for HTTP, WebSocket, and TCP.
- Defaults all apps to `offline_only = true`.

### GuiBroker
- Owns the host-rendered window model.
- Decouples guest drawing/event requests from Chromium iframes.

## Intended host bindings for guest programs

```rust
pub trait WasmOsHost {
    fn vfs_open(&mut self, task_id: &str, path: &str) -> Result<u32, HostError>;
    fn vfs_read(&mut self, fd: u32, len: usize) -> Result<Vec<u8>, HostError>;
    fn vfs_write(&mut self, fd: u32, data: &[u8]) -> Result<(), HostError>;
    fn vfs_readdir(&mut self, path: &str) -> Result<Vec<String>, HostError>;
    fn net_http_fetch(&mut self, request: HttpRequest) -> Result<HttpResponse, HostError>;
    fn net_ws_connect(&mut self, url: &str) -> Result<u32, HostError>;
    fn gui_window_open(&mut self, desc: WindowOpen) -> Result<String, HostError>;
    fn gui_draw_text(&mut self, window_id: &str, text: &str) -> Result<(), HostError>;
    fn scheduler_yield(&mut self) -> Result<(), HostError>;
    fn proc_exit(&mut self, code: i32) -> Result<(), HostError>;
}
```

These bindings can be exposed through preview2/WIT worlds or through low-level Wasmtime linker functions.
