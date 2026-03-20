#!/usr/bin/env node
/**
 * Minimal package registry host for WasmOS.
 *
 * Usage:
 *   npm install express
 *   node tools/pkg_registry_server.js
 */

const express = require("express");
const path = require("path");
const fs = require("fs");

const app = express();
const PORT = process.env.PKG_PORT || 4010;
const programsDir = path.join(__dirname, "programs");
const catalogPath = path.join(__dirname, "packages.json");

if (!fs.existsSync(programsDir)) {
  fs.mkdirSync(programsDir, { recursive: true });
}

if (!fs.existsSync(catalogPath)) {
  fs.writeFileSync(
    catalogPath,
    JSON.stringify(
      {
        "text-editor": {
          module_path: `http://127.0.0.1:${PORT}/programs/text_editor.wasm`,
          dependencies: [],
          version: "1.0.0",
        },
      },
      null,
      2
    )
  );
}

app.get("/packages.json", (_req, res) => {
  const payload = fs.readFileSync(catalogPath, "utf-8");
  res.type("application/json").send(payload);
});

app.use("/programs", express.static(programsDir));

app.listen(PORT, () => {
  console.log(`WasmOS package registry listening on http://127.0.0.1:${PORT}`);
  console.log(`Catalog: http://127.0.0.1:${PORT}/packages.json`);
});
