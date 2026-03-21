# WasmOS

WasmOS is a virtual operating system runtime that runs WebAssembly programs on top of a host OS.
It is organized in explicit layers:

1. **Shell / CLI**
2. **Task Scheduler**
3. **WASM Runtime (Wasmtime)**
4. **Subsystems (VFS / Networking / GUI)**
5. **Host Bridge**

## Repository layout

- `src/shell`: command parser/dispatcher for launching and controlling tasks, including persistent per-program policy profiles.
- `src/scheduler`: ready/wait queues, task states, cooperative + preemptive scheduling.
- `src/runtime`: Wasmtime-based loading/instantiation/resume, ABI checks, fuel accounting.
- `src/program_api`: host function registrations exposed as `wasmos::*` imports.
- `src/vfs`: in-memory virtual filesystem and per-task access policy.
- `src/network`: sandboxed network capabilities (HTTP/WebSocket/TCP) with per-task policy.
- `src/gui`: host-rendered windowing and drawing abstractions.
- `src/host`: host capability detection and boundary.
- `docs/abi.md`: versioned guest ABI contract.
- `guest_abi`: shared guest-side ABI structs/constants.
- `crates/wasmos-host`: standalone Web OS host service that exposes the desktop shell, HTTP API, and kiosk integration path.
- `webos/`: local desktop shell plus HTML/JS app bundles mounted into Chromium kiosk mode.
- `buildroot/`: boot assets, GRUB config, init scripts, and rootfs overlay for the bootable ISO profile.

## Build and run

```bash
cargo build
cargo run
```

Optional desktop GUI backend:

```bash
cargo run --features desktop-gui
```

Standalone Web OS host service:

```bash
cargo run --manifest-path crates/wasmos-host/Cargo.toml
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
- `pkg update`: refresh local package catalog from all registered hosts
- `pkg install <program>`
- `pkg upgrade`: upgrade all installed programs to latest catalog versions
- `pkg upgrade <program>`: upgrade one program
- `pkg remove <program>`
- `pkg list`

Installed packages can be executed directly by name.

### Program-scoped policies

Network policies can be stored per program so toggling one package does not affect others:

- `NP show -p <program>`
- `NP set [http: on, remote: on] -p <program>`
- `NP set [all: true] -p <program>` enables all network capabilities and clears host-level restrictions
- `NP show -t <task_id>`
- `NP set [http: on, remote: on] -t <task_id>`

Those settings are persisted in `.wasmos_program_policies.json` and are applied automatically each time that program is launched.

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

## Web OS boot profile

The new Web OS profile boots a minimal Linux image straight into a fully local Chromium kiosk session.

- `buildroot/board/newos/grub/grub.cfg` auto-loads the kernel/initrd with no menu and supports a splash asset.
- `buildroot/board/newos/rootfs-overlay/etc/init.d/S99newos` starts the local host service and Chromium kiosk automatically.
- `buildroot/board/newos/rootfs-overlay/usr/local/bin/newos-kiosk` launches Chromium with low-memory kiosk flags.
- `buildroot/board/newos/rootfs-overlay/usr/local/bin/newos-server` runs the Rust HTTP host that serves `/desktop`, `/apps`, `/api/*`, and app-scoped storage.
- `webos/apps/` is the runtime app directory; apps load dynamically at runtime inside sandboxed iframes.
- `docs/architecture.md` documents the Shell → Scheduler → RuntimeHost → VFS/Net/GUI → Host OS contract.
- `docs/iso-packaging.md` and `scripts/package-iso.sh` document the ISO packaging and QEMU test flow.

### App sandboxing

- Each app gets `/apps/<app-id>` and `/data/apps/<app-id>` capability roots.
- Path traversal is rejected before host filesystem access is granted.
- Apps render in sandboxed Chromium iframes and communicate only through the local host API.
- Network policy defaults to offline-only until an app-specific allowlist is configured.

## Safety model

- Guests run in Wasmtime sandboxes and cannot access host memory directly.
- All file/network/gui operations are capability-gated host calls.
- Runtime scheduling uses explicit fuel budgets per quantum for preemptive execution.
