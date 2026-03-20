# WasmOS Guest Programs

Real minimal guest programs that exercise the `wasmos::*` ABI.

Shared ABI request/response structs and status constants are provided by `../guest_abi` (`wasmos_guest_abi`).

## Build

```bash
cargo build --target wasm32-wasip1 --release
```

Artifacts are emitted under:

- `target/wasm32-wasip1/release/cli_welcome.wasm`
- `target/wasm32-wasip1/release/file_ops.wasm`
- `target/wasm32-wasip1/release/http_fetch.wasm`
- `target/wasm32-wasip1/release/gui_window.wasm`
- `target/wasm32-wasip1/release/cooperative_yielder.wasm`

These can be launched from WasmOS shell with:

```text
run <path-to-wasm>
```
