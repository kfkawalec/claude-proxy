#!/usr/bin/env node
/**
 * Regeneruje ikony aplikacji z jednego źródła SVG + ikony tray (Resvg + tray-icon.svg).
 *
 * Pełny zestaw: `npm run icons` (domyślnie, bez argumentów)
 * — `src/assets/favicon.svg` → `tauri icon` (32, 128, @2x, icon.png, .ico, .icns),
 * — tray z `src-tauri/icons/tray-icon.svg` + win-dark.
 *
 * Tylko tray: `npm run icons:tray` → `node scripts/rebuild-icons.mjs tray`
 */
import { execSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Resvg } from "@resvg/resvg-js";
import { PNG } from "pngjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, "..");
const sub = process.argv[2];

/** @param {string} root */
function renderTrayIcons(root) {
  const traySvg = path.join(root, "src-tauri", "icons", "tray-icon.svg");
  const outDir = path.join(root, "src-tauri", "icons");
  const outTray = path.join(outDir, "tray-icon.png");
  const outWin = path.join(outDir, "tray-icon-win-dark.png");

  if (!fs.existsSync(traySvg)) {
    console.error("Brak pliku:", traySvg);
    process.exit(1);
  }

  const svg = fs.readFileSync(traySvg);
  const opts = {
    background: "rgba(0,0,0,0)",
    fitTo: { mode: "width", value: 44 },
    font: { loadSystemFonts: false },
  };
  const png = new Resvg(svg, opts).render().asPng();
  fs.writeFileSync(outTray, png);
  console.log("→ icons/", path.basename(outTray));
  fs.writeFileSync(outWin, makeWinDarkPng(png));
  console.log("→ icons/", path.basename(outWin));
  console.log("tray render: resvg + tray-icon.svg (44px, Retina)");
}

/** @param {Buffer} pngBuffer */
function makeWinDarkPng(pngBuffer) {
  const png = PNG.sync.read(pngBuffer);
  const { width: w, height: h, data } = png;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const idx = (w * y + x) << 2;
      const r = data[idx];
      const g = data[idx + 1];
      const b = data[idx + 2];
      const a = data[idx + 3];
      if (a < 12) continue;
      const lum = (r + g + b) / 3;
      if (lum < 90) {
        data[idx] = 255;
        data[idx + 1] = 255;
        data[idx + 2] = 255;
      } else {
        const t = Math.max(0, 1 - (lum - 90) / 165);
        data[idx] = 255;
        data[idx + 1] = 255;
        data[idx + 2] = 255;
        data[idx + 3] = Math.floor(a * t);
      }
    }
  }
  return PNG.sync.write(png);
}

if (sub === "tray") {
  renderTrayIcons(root);
  console.log("Gotowe: tray.");
  process.exit(0);
}

if (sub != null && sub !== "") {
  console.error('Nieznany argument. Użyj: node scripts/rebuild-icons.mjs  |  node scripts/rebuild-icons.mjs tray');
  process.exit(1);
}

const svg = path.join(root, "src", "assets", "favicon.svg");
const tauriDir = path.join(root, "src-tauri");
const outIcons = path.join(tauriDir, "icons");

const BUNDLE_FILES = [
  "32x32.png",
  "128x128.png",
  "128x128@2x.png",
  "icon.png",
  "icon.ico",
  "icon.icns",
];

if (!fs.existsSync(svg)) {
  console.error("Brak pliku:", svg);
  process.exit(1);
}

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "claude-proxy-icons-"));
try {
  const relSvg = path.relative(tauriDir, svg);
  const localTauri = path.join(root, "node_modules", ".bin", "tauri");
  const runner = fs.existsSync(localTauri)
    ? `"${localTauri}"`
    : "npx --yes @tauri-apps/cli@2";
  execSync(`${runner} icon "${relSvg}" -o "${tmp}"`, {
    cwd: tauriDir,
    stdio: "inherit",
    env: process.env,
    shell: true,
  });
} catch {
  fs.rmSync(tmp, { recursive: true, force: true });
  process.exit(1);
}

for (const f of BUNDLE_FILES) {
  const from = path.join(tmp, f);
  const to = path.join(outIcons, f);
  if (!fs.existsSync(from)) {
    console.error("Brak wygenerowanego pliku:", f);
    process.exit(1);
  }
  fs.copyFileSync(from, to);
  console.log("→ icons/", f);
}

fs.rmSync(tmp, { recursive: true, force: true });

renderTrayIcons(root);

console.log("Gotowe: bundle z favicon.svg + tray z tray-icon.svg (Resvg).");
