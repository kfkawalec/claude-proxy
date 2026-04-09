# Claude Proxy

Aplikacja desktopowa (Tauri + SolidJS) działająca jako proxy między Claude Code a dostawcami AI (Anthropic oraz hub zgodny z OpenAI API, np. LiteLLM). W interfejsie nadajesz hubowi **własną nazwę wyświetlaną** (nie musi to być słowo „LiteLLM”). Możesz przełączać provider bez restartu Claude Code.

## Architektura

- **`src/`** - frontend SolidJS (panel, ikona w trayu)
- **`src-tauri/`** - backend Rust (Tauri), bundlowanie do `.app` / `.exe` / AppImage

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
open src-tauri/target/release/bundle/macos/ClaudeProxy.app
```

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

- **Plik proxy (żądania, upstream):**
  `~/.config/claude-proxy/proxy.log` na macOS/Linux; na Windows zwykle `%USERPROFILE%\.config\claude-proxy\proxy.log`.

- **macOS - strumień logów systemowych (przykład):**

```bash
log stream --predicate 'process == "claude-proxy"' --level debug
```
