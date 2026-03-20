# WasmOS Package Installation

WasmOS supports local and remote package catalogs.

## Catalog format

Package host endpoint: `GET /packages.json`

Example payload:

```json
{
  "text-editor": {
    "module_path": "http://127.0.0.1:4010/programs/text_editor.wasm",
    "dependencies": ["core-utils"]
  },
  "core-utils": {
    "module_path": "http://127.0.0.1:4010/programs/core_utils.wasm",
    "dependencies": []
  }
}
```

## Shell commands

- `pkg host add http://127.0.0.1:4010`
- `pkg host list`
- `pkg install text-editor`
- `pkg list`
- `pkg remove text-editor`

Installed package names become executable commands in the shell.

## Local persistence

- `.wasmos_packages.json` stores installed packages.
- `.wasmos_pkg_hosts.json` stores configured package hosts.
- `.wasmos_pkg_catalog.json` can be used as a local fallback catalog.
