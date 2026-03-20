# WasmOS Architecture

## Layering

1. **Shell / CLI**: parses commands, launches modules, and reports scheduler/runtime state.
2. **Scheduler**: manages task metadata, queuing, and the execution policy boundary.
3. **WASM Runtime**: loads WebAssembly modules with Wasmtime, provisions persistent per-task stores/instances, and links guest syscalls.
4. **Subsystem APIs**: VFS, networking, and GUI services exposed to guest programs via imported host functions.
5. **Host Bridge**: abstracts whether WasmOS is hosted on desktop or browser surfaces.

## Modules

- `src/shell`: interactive CLI surface and command dispatch, including staging/execution split commands (`spawn`, `resume`, `runq`, `run`) and system inspection commands (`ls`, `cat`, `write`, `rm`, `mkdir`, `mount`, `kill`, `logs`, `net policy`, `window list`).
- `src/scheduler`: task control blocks, run queue, and scheduling modes.
- `src/runtime`: Wasmtime engine, persistent task instance lifecycle, and runtime context wiring.
- `src/program_api`: syscall registration plus guest-facing API skeletons.
- `docs/abi.md`: versioned guest syscall contract (function names, wire structs, and error code table).
- `guest_abi`: shared guest-side crate exporting ABI request/response structs, open flags, syscall names, and `OsErrorCode`.
- `src/vfs`: in-memory filesystem nodes plus per-task permissions, descriptor-based open/read/write/seek, and directory management syscalls.
- `src/network`: sandboxed HTTP/WebSocket/TCP policy enforcement with host-backed adapters and per-task session managers.
- `src/gui`: command-driven window manager with host backend adapters (desktop renderer feature + in-memory fallback), draw interpreter, event queue, and text rendering abstraction.
- `src/host`: host capability detection and abstraction boundary.

## Safety model

- Wasmtime memory isolation prevents guests from directly accessing host memory.
- Resource access flows only through `RuntimeContext` and linked imports.
- `NetworkSubsystem` enforces per-task policies before network operations are forwarded.
- `VirtualFileSystem` decouples virtual paths from host paths and only exposes mapped host metadata explicitly.
- VFS operations are authorized per task through explicit read/write/create/delete policy gates before file descriptor operations execute.
- GUI access is command/event based, so guests cannot access host hardware directly.
- Frame redraw/invalidation scheduling is backend-owned and only triggered through explicit draw commands.
- Desktop rendering uses a dedicated GUI thread with channel-based command/event handoff, avoiding cross-thread ownership of non-`Send` window handles.

## Unified error model

- `src/os_error` defines a shared `OsErrorCode` (numeric errno-style values) and `OsError` structure.
- Subsystems convert internal errors to `OsError` (`From<VfsError>`, `From<anyhow::Error>`), and ABI wrappers map those codes directly to guest-visible syscall results.
- This keeps host internals expressive while giving guests stable numeric status codes for program control-flow.


## ABI strategy

WasmOS now models guest execution as a **hybrid WASI + custom import** system:

- **WASI (`wasi_snapshot_preview1`)** remains the default process ABI for standard entrypoints, argv/env delivery, and stdio-style behavior.
- **Custom `wasmos::*` imports** remain the capability boundary for the virtual filesystem, policy-gated networking, and GUI/windowing services.
- **Per-program ABI selection** is explicit through `ProgramLaunchRequest.abi`, which supports:
  - `PureWasi` for CLI-style programs that only need WASI behavior.
  - `CustomOnly` for sandboxed programs that should avoid direct WASI imports.
  - `Hybrid` for programs that combine WASI process semantics with WasmOS-specific capabilities.
- The runtime validates module imports against the selected ABI strategy before instantiation, preventing accidental mixing of unsupported interfaces.


## Multitasking scheduler model

The scheduler now models multitasking as a set of explicit queues and runtime poll states:

- **Ready queue**: runnable tasks waiting for a CPU quantum.
- **Waiting queue**: tasks blocked on sleep deadlines or named I/O/event channels.
- **Clock tick**: each scheduler tick advances a logical timer used to wake sleeping tasks.
- **Runtime poll contract**: the Wasm runtime returns `Ready`, `Yielded`, `Waiting(...)`, or `Exited(...)` so the scheduler can requeue or block tasks instead of treating every run as a one-shot process.
- **Persistent task state**: every launched task keeps a resident `Store` + `Instance` pair, preserving linear memory, globals, and in-guest data between scheduler quanta.
- **Resumable guest contract**: custom-only guests expose `wasmos_resume`, while WASI-oriented guests may use `_start`/`main`; this separates one-shot process semantics from resumable cooperative tasks.
- **Cooperative/preemptive split**: cooperative mode uses a zero-length quantum marker, while preemptive mode carries an explicit `quantum_ms` value into the runtime control block for future fuel/timer enforcement.
- **Capability front doors**: VFS and networking requests from guests always pass through syscall policy checks (`TaskVfsPolicy`, `SocketPolicy`) before any host-backed operation is attempted.

This keeps the architecture aligned with a future resident-task runtime while already separating ready, sleeping, and event-waiting work in the scheduler.

## Expansion points

- Swap in a real rendering backend behind `GuiSubsystem::draw`.
- Back `NetworkSubsystem` with concrete host adapters for HTTP, WebSocket, and TCP.
- Add real WASI previews or a custom ABI layer in `program_api`.
- Extend `Scheduler::tick` into multi-quantum or multi-core orchestration.
