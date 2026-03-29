#!/usr/bin/env bash
# Składa latest.json (Tauri Updater) z dostępnych katalogów updater-linux / updater-windows / updater-macos.
# Brak katalogu albo niekompletny zestaw (paczka + podpis; na macOS dodatkowo platform.txt) = pominięcie platformy (komunikat na stderr).
# Gdy żadna platforma nie jest gotowa — exit 1.
#
# Wymaga PUBLIC_ASSET_BASE_URL — baza publicznych URL-i do plików, np.:
#   https://github.com/owner/repo/releases/download/v1.0.0
#
# Użycie: ./scripts/updater-manifest.sh [updater-linux] [updater-windows] [updater-macos]
# Domyślnie: updater-linux updater-windows updater-macos
#
# UPDATER_ASSET_URL_MODE:
#   flat  — pliki pod BASE/nazwa (GitHub Release; domyślnie flat)
#   subdir — pliki pod BASE/updater/nazwa

set -euo pipefail

if [[ -z "${PUBLIC_ASSET_BASE_URL:-}" ]]; then
  echo "Ustaw PUBLIC_ASSET_BASE_URL (np. https://github.com/o/r/releases/download/v1.0.0)" >&2
  exit 1
fi

PUBLIC_BASE="${PUBLIC_ASSET_BASE_URL}"
VERSION=$(node -p "require('./package.json').version")
PUB_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

L_DIR="${1:-updater-linux}"
W_DIR="${2:-updater-windows}"
M_DIR="${3:-updater-macos}"

if [[ "${UPDATER_ASSET_URL_MODE:-flat}" == "flat" ]]; then
  PUB="${PUBLIC_BASE%/}"
else
  PUB="${PUBLIC_BASE%/}/updater"
fi

L_APP=""
L_SIG=""
if [[ ! -d "$L_DIR" ]]; then
  echo "Pomijam Linux: brak katalogu $L_DIR" >&2
else
  L_APP=$(find "$L_DIR" -maxdepth 1 -type f \( -name '*.AppImage' -o -name '*.appimage' \) ! -name '*.sig' 2>/dev/null | head -1)
  L_SIG=$(find "$L_DIR" -maxdepth 1 -type f -name '*.sig' 2>/dev/null | head -1)
  if [[ -z "$L_APP" || -z "$L_SIG" ]]; then
    echo "Pomijam Linux: brak AppImage + .sig w $L_DIR" >&2
    L_APP=""
    L_SIG=""
  fi
fi

W_APP=""
W_SIG=""
if [[ ! -d "$W_DIR" ]]; then
  echo "Pomijam Windows: brak katalogu $W_DIR" >&2
else
  W_APP=$(find "$W_DIR" -maxdepth 1 -type f -name '*setup.exe' 2>/dev/null | head -1)
  if [[ -z "$W_APP" ]]; then
    W_APP=$(find "$W_DIR" -maxdepth 1 -type f -name '*.exe' 2>/dev/null | head -1)
  fi
  W_SIG=$(find "$W_DIR" -maxdepth 1 -type f -name '*.sig' 2>/dev/null | head -1)
  if [[ -z "$W_APP" || -z "$W_SIG" ]]; then
    echo "Pomijam Windows: brak .exe + .sig w $W_DIR" >&2
    W_APP=""
    W_SIG=""
  fi
fi

M_APP=""
M_SIG=""
DARWIN_KEY=""
if [[ ! -d "$M_DIR" ]]; then
  echo "Pomijam macOS: brak katalogu $M_DIR" >&2
else
  M_APP=$(find "$M_DIR" -maxdepth 1 -type f -name '*.app.tar.gz' 2>/dev/null | head -1)
  if [[ -z "$M_APP" ]]; then
    M_APP=$(find "$M_DIR" -maxdepth 1 -type f -name '*.tar.gz' ! -name '*.sig' 2>/dev/null | head -1)
  fi
  M_SIG=$(find "$M_DIR" -maxdepth 1 -type f -name '*.sig' 2>/dev/null | head -1)
  if [[ -f "$M_DIR/platform.txt" ]]; then
    DARWIN_KEY=$(tr -d '\r\n' < "$M_DIR/platform.txt")
  fi
  if [[ -z "$M_APP" || -z "$M_SIG" || -z "$DARWIN_KEY" ]]; then
    echo "Pomijam macOS: brak .app.tar.gz / .sig / platform.txt w $M_DIR" >&2
    M_APP=""
    M_SIG=""
    DARWIN_KEY=""
  fi
fi

if [[ -z "$L_APP" && -z "$W_APP" && -z "$M_APP" ]]; then
  echo "Brak żadnej kompletnej platformy do manifestu." >&2
  exit 1
fi

L_URL=""
[[ -n "$L_APP" ]] && L_URL="${PUB}/$(basename "$L_APP")"
W_URL=""
[[ -n "$W_APP" ]] && W_URL="${PUB}/$(basename "$W_APP")"
M_URL=""
[[ -n "$M_APP" ]] && M_URL="${PUB}/$(basename "$M_APP")"

OUT="${MANIFEST_OUTPUT:-updater/latest.json}"
mkdir -p "$(dirname "$OUT")"
NOTES="${MANIFEST_NOTES:-CI build}"

export MANIFEST_L_SIG="$L_SIG"
export MANIFEST_W_SIG="$W_SIG"
export MANIFEST_M_SIG="$M_SIG"
export MANIFEST_DARWIN_KEY="$DARWIN_KEY"
export MANIFEST_VERSION="$VERSION"
export MANIFEST_PUB_DATE="$PUB_DATE"
export MANIFEST_NOTES_INLINE="$NOTES"
export MANIFEST_OUTPUT_PATH="$OUT"
export MANIFEST_L_URL="$L_URL"
export MANIFEST_W_URL="$W_URL"
export MANIFEST_M_URL="$M_URL"

node -e "
const fs = require('fs');
const platforms = {};
const lSig = process.env.MANIFEST_L_SIG || '';
const wSig = process.env.MANIFEST_W_SIG || '';
const mSig = process.env.MANIFEST_M_SIG || '';
const dk = process.env.MANIFEST_DARWIN_KEY || '';
const lUrl = process.env.MANIFEST_L_URL || '';
const wUrl = process.env.MANIFEST_W_URL || '';
const mUrl = process.env.MANIFEST_M_URL || '';
if (lSig && lUrl) {
  platforms['linux-x86_64'] = { signature: fs.readFileSync(lSig, 'utf8').trim(), url: lUrl };
}
if (wSig && wUrl) {
  platforms['windows-x86_64'] = { signature: fs.readFileSync(wSig, 'utf8').trim(), url: wUrl };
}
if (mSig && mUrl && dk) {
  platforms[dk] = { signature: fs.readFileSync(mSig, 'utf8').trim(), url: mUrl };
}
if (Object.keys(platforms).length === 0) {
  console.error('Brak platform w manifeście (wewnętrzny błąd).');
  process.exit(1);
}
const j = {
  version: process.env.MANIFEST_VERSION,
  notes: process.env.MANIFEST_NOTES_INLINE || 'CI build',
  pub_date: process.env.MANIFEST_PUB_DATE,
  platforms,
};
fs.writeFileSync(process.env.MANIFEST_OUTPUT_PATH, JSON.stringify(j, null, 2) + '\\n');
"

echo "Zapisano $OUT"
cat "$OUT"
