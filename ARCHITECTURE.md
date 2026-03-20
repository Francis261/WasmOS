# WasmOS Architecture

## Layering

1. **Shell / CLI**: parses commands, launches modules, and reports scheduler/runtime state.
2. **Scheduler**: manages task metadata, queuing, and the execution policy boundary.
3. **WASM Runtime**: loads WebAssembly modules with Wasmtime, provisions stores, and links guest syscalls.
4. **Subsystem APIs**: VFS, networking, and GUI services exposed to guest programs via imported host functions.
5. **Host Bridge**: abstracts whether WasmOS is hosted on desktop or browser surfaces.

## Modules

- `src/shell`: interactive CLI surface and command dispatch.
- `src/scheduler`: task control blocks, run queue, and scheduling modes.
- `src/runtime`: Wasmtime engine, per-task instantiation, and runtime context wiring.
- `src/program_api`: syscall registration plus guest-facing API skeletons.
- `src/vfs`: in-memory filesystem nodes with optional host path mapping metadata.
- `src/network`: sandboxed HTTP/WebSocket/TCP policy enforcement and adapters.
- `src/gui`: host-rendered window, draw-command, and event abstractions.
- `src/host`: host capability detection and abstraction boundary.

## Safety model

- Wasmtime memory isolation prevents guests from directly accessing host memory.
- Resource access flows only through `RuntimeContext` and linked imports.
- `NetworkSubsystem` enforces per-task policies before network operations are forwarded.
- `VirtualFileSystem` decouples virtual paths from host paths and only exposes mapped host metadata explicitly.
- GUI access is command/event based, so guests cannot access host hardware directly.


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
- **Cooperative/preemptive split**: cooperative mode uses a zero-length quantum marker, while preemptive mode carries an explicit `quantum_ms` value into the runtime control block for future fuel/timer enforcement.

This keeps the architecture aligned with a future resident-task runtime while already separating ready, sleeping, and event-waiting work in the scheduler.

## Expansion points

- Swap in a real rendering backend behind `GuiSubsystem::draw`.
- Back `NetworkSubsystem` with concrete host adapters for HTTP, WebSocket, and TCP.
- Add real WASI previews or a custom ABI layer in `program_api`.
- Extend `Scheduler::tick` into multi-quantum or multi-core orchestration.
