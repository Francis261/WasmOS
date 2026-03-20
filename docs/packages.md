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
- `pkg host remove http://127.0.0.1:4010`
- `pkg host list`
- `pkg update` (checks all hosts, marks each host `[ok]` or `[bad]`, refreshes local catalog)
- `pkg install text-editor`
- `pkg list`
- `pkg upgrade` (upgrade all installed packages that have newer versions)
- `pkg upgrade text-editor` (upgrade one package)
- `pkg remove text-editor`

Installed package names become executable commands in the shell.

## Local persistence

- `.wasmos_packages.json` stores installed packages.
- `.wasmos_pkg_hosts.json` stores configured package hosts.
- `.wasmos_pkg_catalog.json` can be used as a local fallback catalog.

## Versioning behavior

- Catalog entries include a `version` field.
- `pkg update` refreshes package metadata only.
- `pkg upgrade` applies metadata updates to installed programs.
- Up-to-date packages are skipped with an explicit message.
