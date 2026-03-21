# Runtime architecture reference

## Layer separation

```text
Shell / CLI
  -> Scheduler
    -> WASM Runtime (Wasmtime/WASI + host functions)
      -> VFS / Networking / GUI capability services
        -> Host OS bridge (Node backend, browser desktop, Linux services)
```

## Module responsibilities

### `runtime/src/shell`
- Parse command lines.
- Resolve built-in shell commands.
- Submit runnable tasks to the scheduler.
- Request WASM preparation from the runtime host.

### `runtime/src/scheduler`
- Maintain task control blocks.
- Provide cooperative round-robin scheduling hooks.
- Track PID, state, and execution ticks.
- Surface extension points for preemption/timers.

### `runtime/src/wasm`
- Own the Wasmtime engine and linker.
- Bind WASI plus custom host capability interfaces.
- Prepare isolated stores for each task.
- Mediate VFS/network/GUI access so guest modules never access host memory directly.

### `runtime/src/vfs`
- Provide an in-memory file tree.
- Support host-backed mounts as a future extension.
- Return opaque data to the runtime host instead of raw host pointers.

### `runtime/src/net`
- Enforce URL and protocol authorization.
- Support separate capability gates for HTTP, WebSocket, and TCP.
- Centralize policy updates for each app or tenant.

### `runtime/src/gui`
- Accept GUI commands from guest programs.
- Represent windows, draw requests, and event channels.
- Relay drawing/event packets to the host renderer.

## Guest API model

Recommended guest-facing syscalls or component interfaces:

- `fs_open(path, flags) -> fd`
- `fs_read(fd, len) -> bytes`
- `fs_write(fd, bytes) -> written`
- `fs_list(path) -> list<string>`
- `net_request(method, url, headers, body) -> response`
- `net_websocket_connect(url) -> socket_id`
- `gui_window_open(title, width, height) -> window_id`
- `gui_draw_text(window_id, x, y, text)`
- `gui_draw_pixels(window_id, width, height, rgba)`
- `sched_yield()`
- `proc_spawn(module, argv) -> pid`

## Browser bridge strategy

The browser desktop should not run WASM tasks with direct host permissions. Instead:

1. A local app asks the backend to execute a module.
2. The backend validates the app manifest and storage scope.
3. The backend forwards the request to the Rust runtime daemon or CLI.
4. The runtime executes the WASM module in an isolated store with explicit capabilities.
5. GUI output is transformed into renderer-safe commands for the desktop host.
