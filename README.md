# Claude Proxy

Aplikacja desktopowa (Tauri + SolidJS) działająca jako proxy między Claude Code a dostawcami AI (Anthropic oraz hub zgodny z OpenAI API, np. LiteLLM). W interfejsie nadajesz hubowi **własną nazwę wyświetlaną** (nie musi to być słowo „LiteLLM”). Możesz przełączać provider bez restartu Claude Code.

## Architektura

- **`src/`** - frontend SolidJS (panel, ikona w trayu)
- **`src-tauri/`** - backend Rust (Tauri), bundlowanie do `.app` / `.exe` / AppImage

### Ikony (aplikacja + tray)

- **Źródło grafiki aplikacji** (logo z tłem, gradient): [`src/assets/favicon.svg`](src/assets/favicon.svg).
- **Regeneracja** wszystkich plików z `bundle.icon` w `tauri.conf.json` (`32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.png`, `icon.ico`, `icon.icns`) oraz **tray** (`tray-icon.png`, `tray-icon@2x.png`, `tray-icon-win-dark.png`):

```bash
npm run icons
```

Używa lokalnego `@tauri-apps/cli` (`tauri icon`) do katalogu tymczasowego — w repozytorium zostają tylko potrzebne pliki (bez wygenerowanych katalogów iOS/Android). Ikony tray (`tray-icon*.png`) rasteruje **`src-tauri/icons/tray-icon.svg`** przez `@resvg/resvg-js` (te same proporcje co glif w favicon). Tylko tray: `npm run icons:tray` (to samo co `node scripts/rebuild-icons.mjs tray`).

## Wymagania

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Zależności JS
npm install
```

## Uruchomienie

### Dev (hot reload)

```bash
npm run tauri dev
```

Vite + aplikacja Tauri. Ikona w obszarze powiadomień / pasku menu. Logi Rust w terminalu.

### Produkcja - kompilacja

```bash
npm run tauri build
```

Powstaje m.in. `.app` oraz (na macOS) `.dmg`. Tauri **nadal** generuje i uruchamia `bundle_dmg.sh` w katalogu `src-tauri/target/release/bundle/dmg/` — to nie jest plik w repozytorium, tylko artefakt buildu.

Jeśli krok DMG kończy się błędem `bundle_dmg.sh` (często brak zgody na **Automatyzację** Findera dla terminala), użyj:

```bash
npm run tauri:build:dmg-safe
```

To **ten sam** skrypt `bundle_dmg.sh`, ale z opcją `--skip-jenkins`: **pomija** układanie ikon w oknie DMG przez AppleScript. Sam plik `.dmg` wygląda wtedy „gościej”, ale **`.app` jest taki sam**. Pełny wygląd DMG wraca po zwykłym `npm run tauri build` i ustawieniu uprawnień (Ustawienia → Prywatność → Automatyzacja).

```bash
open src-tauri/target/release/bundle/macos/ClaudeProxy.app
```

## Wersjonowanie i release

- **Źródło prawdy:** pole `version` w `package.json`. Skrypt `scripts/sync-version.cjs` ustawia tę samą wersję w `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` oraz w wpisie pakietu w `src-tauri/Cargo.lock`.
- **Podbicie wersji (bez commita i tagu lokalnego):**

```bash
npm run bump          # patch, np. 0.1.1 → 0.1.2
npm run bump:minor    # minor
npm run bump:major    # major
```

Po `bump` zrób commit ze zmianami i push na `main`. Workflow **GitHub Actions** (`.github/workflows/release.yml`) zbuduje artefakty i utworzy release z tagiem **`v<wersja>`** (z prefiksem `v`). Nie twórz ręcznie drugiego tagu w formacie `0.1.0` bez `v` — powstanie duplikat względem CI.

Ręczna edycja `package.json` jest możliwa; wtedy jednorazowo uruchom: `node scripts/sync-version.cjs`.

## Konfiguracja Claude Code

Proxy nasłuchuje na porcie **3456**.

**Opcje:**

1. **Z aplikacji** - w lewej kolumnie sekcja **Claude Code**: instalacja zapisuje `ANTHROPIC_BASE_URL` w `~/.claude/settings.json` (z kopią zapasową poprzedniej wartości). Nie trzeba edytować shella ręcznie.
2. **Ręcznie w shellu** - np. w `~/.zshrc`:

```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:3456
```

## Uwierzytelnianie

### Claude (Anthropic)

Proxy **nie wstawia własnego klucza** - przekazuje nagłówki z Claude Code (`x-api-key` / `Authorization`). Musisz być zalogowany w **Claude Code** (np. `claude auth login`).

**Jak sprawdzić:**

- W terminalu: `claude auth status` - w JSON m.in. pole `loggedIn`.
- W aplikacji: lewa kolumna **Claude Code** - kropka statusu i przycisk logowania (CLI otwiera terminal z `claude auth login`; szczegóły przechowywania OAuth zależą od systemu, na macOS często Keychain).

### Hub (OpenAI-compatible / LiteLLM)

Przy wybranym hubie w **Settings** ustaw **nazwę wyświetlaną**, **URL** i **klucz API**. Mapowanie **Claude → modele upstream** (Opus / Sonnet / Haiku) jest w tej samej zakładce.

Zakładka **Stats** pokazuje m.in. lokalne sumy zapytań/tokenów przez proxy, limity planu Claude (gdy dotyczy) oraz zużycie z huba (API - m.in. okres „ostatni miesiąc”, modele), o ile klucz ma wymagane uprawnienia.

## Logi

### Dev

Logi Rust w terminalu uruchomionym z `npm run tauri dev`.

### Produkcja

- **Plik proxy (żądania, upstream):** domyślnie dopisywany do
  `~/.config/claude-proxy/proxy.log` (macOS/Linux) lub `%USERPROFILE%\.config\claude-proxy\proxy.log` (Windows).
  Wyłączenie: `CLAUDE_PROXY_FILE_LOG=0` (wtedy same linie idą na stdout — przy apce z Docka na macOS zwykle „nigdzie”).
- **Pełne body żądania** (do porównań / debugu): `CLAUDE_PROXY_LOG_REQUEST_BODY=1` (opcjonalnie `CLAUDE_PROXY_LOG_REQUEST_BODY_MAX` w bajtach).

- **macOS - strumień logów systemowych (przykład):**

```bash
log stream --predicate 'process == "claude-proxy"' --level debug
```
