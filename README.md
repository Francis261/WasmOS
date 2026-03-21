# WasmOS / Web OS Architecture

This repository contains a bootable Web OS architecture that launches a local Chromium desktop and embeds a sandboxed WASM-centric runtime. The design is intentionally split into layers so the same runtime can operate inside a booted ISO or atop an existing Linux host.

## Layered architecture

1. **Boot layer** – GRUB boots a minimal Linux kernel and initrd directly into the kiosk environment.
2. **Init / service layer** – startup scripts mount writable storage, start the local backend, then launch Chromium in kiosk mode.
3. **Web desktop layer** – local HTML/CSS/JS renders the desktop shell, app launcher, file manager, notes, calculator, and settings.
4. **Runtime services** – a local Node.js backend enforces app sandboxes for `/apps` and `/data`, exposes secure APIs, and can bridge to the Rust WASM runtime.
5. **WASM OS layer** – the Rust runtime provides the shell, scheduler, VFS, networking, GUI host bridge, and WASM execution environment.

## Repository layout

- `boot/grub/grub.cfg` – GRUB configuration for unattended boot.
- `scripts/` – startup, packaging, QEMU test, and Chromium launch scripts.
- `server/` – local backend with strict path controls and app-aware file APIs.
- `web/` – offline desktop shell and local apps served from disk.
- `runtime/` – Rust code skeleton for the WASM-based virtual OS runtime.
- `docs/iso-packaging.md` – ISO packaging and testing instructions.

## Boot flow summary

1. GRUB autoloads `vmlinuz` and `initrd.img`.
2. `/init` or the system service invokes `scripts/start-webos.sh`.
3. `start-webos.sh` prepares `/data` and `/apps`, launches the local backend on `127.0.0.1:8080`, then starts Chromium in kiosk mode.
4. Chromium opens `http://127.0.0.1:8080/index.html` and the desktop loads local app manifests dynamically.

## Security model summary

- Apps live under `/apps/<app-id>` and get file access only through backend-scoped routes.
- `/data/apps/<app-id>` is the writable app-private root.
- `/data/shared` is optional shared storage and must be requested explicitly.
- The browser loads apps in sandboxed iframes using `allow-scripts` and `allow-same-origin`; direct host FS access is not exposed.
- The Rust runtime does not expose host memory and uses explicit capability objects for file, network, and GUI resources.

## Quick start

```bash
npm install
cargo check
bash scripts/dev-serve.sh
```

Then open `http://127.0.0.1:8080` locally or run the QEMU workflow from `docs/iso-packaging.md`.
