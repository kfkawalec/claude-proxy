/**
 * Ustawia wersję w src-tauri/Cargo.toml i src-tauri/tauri.conf.json
 * zgodnie z package.json (jedno źródło prawdy).
 */
const fs = require("fs");
const path = require("path");

const root = path.join(__dirname, "..");
const v = require(path.join(root, "package.json")).version;

const cargoPath = path.join(root, "src-tauri", "Cargo.toml");
const lines = fs.readFileSync(cargoPath, "utf8").split("\n");
let i = 0;
while (i < lines.length && lines[i].trim() !== "[package]") i++;
while (i < lines.length) {
  const t = lines[i].trim();
  if (t.startsWith("[") && t !== "[package]") break;
  if (t.startsWith("version =")) {
    lines[i] = `version = "${v}"`;
    break;
  }
  i++;
}
fs.writeFileSync(cargoPath, lines.join("\n"));

const tauriPath = path.join(root, "src-tauri", "tauri.conf.json");
const tauri = JSON.parse(fs.readFileSync(tauriPath, "utf8"));
tauri.version = v;
fs.writeFileSync(tauriPath, JSON.stringify(tauri, null, 2) + "\n");

console.log("sync-version:", v);
