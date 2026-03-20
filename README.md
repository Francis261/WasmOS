# WasmOS

WasmOS is a virtual operating system runtime that runs WebAssembly programs on top of a host OS.
It is organized in explicit layers:

1. **Shell / CLI**
2. **Task Scheduler**
3. **WASM Runtime (Wasmtime)**
4. **Subsystems (VFS / Networking / GUI)**
5. **Host Bridge**

## Repository layout

- `src/shell`: command parser/dispatcher for launching and controlling tasks.
- `src/scheduler`: ready/wait queues, task states, cooperative + preemptive scheduling.
- `src/runtime`: Wasmtime-based loading/instantiation/resume, ABI checks, fuel accounting.
- `src/program_api`: host function registrations exposed as `wasmos::*` imports.
- `src/vfs`: in-memory virtual filesystem and per-task access policy.
- `src/network`: sandboxed network capabilities (HTTP/WebSocket/TCP) with per-task policy.
- `src/gui`: host-rendered windowing and drawing abstractions.
- `src/host`: host capability detection and boundary.
- `docs/abi.md`: versioned guest ABI contract.
- `guest_abi`: shared guest-side ABI structs/constants.

## Build and run

```bash
cargo build
cargo run
```

Optional desktop GUI backend:

```bash
cargo run --features desktop-gui
```

## Build guest wasm binaries

```bash
cd guest_programs
cargo build --target wasm32-wasip1 --release
```

## Shell quick start

From the `wasmos>` prompt:

- `spawn <module.wasm>`: register + prepare a task.
- `resume <task_id>`: run one ready quantum for a task.
- `runq <rounds>`: run scheduler for `rounds`.
- `ps`: inspect task control blocks.

### Prompt style

The shell prompt uses a colored user/host style and reflects current directory:

- root: `wasmos@user:$`
- nested: `wasmos@user:[root/.../current]$` (middle segments are truncated automatically for long paths)

### Package manager

- `pkg host add <url>`: register a remote package host (`<url>/packages.json` is expected)
- `pkg host list`
- `pkg install <program>`
- `pkg remove <program>`
- `pkg list`

Installed packages can be executed directly by name.

### Built-in editor

`Teditor <file-path>` opens the built-in text editor mode.

Editor commands:
- `:w` save
- `:q` quit
- `:wq` save and quit
- `:rename <path>` change output file
- `:select_all`
- `:paste`
- `:help`

## Safety model

- Guests run in Wasmtime sandboxes and cannot access host memory directly.
- All file/network/gui operations are capability-gated host calls.
- Runtime scheduling uses explicit fuel budgets per quantum for preemptive execution.
