# Claude Proxy

Aplikacja desktopowa (Tauri + SolidJS) działająca jako proxy między Claude Code a dostawcami AI (Anthropic oraz hub zgodny z OpenAI API, np. wdrożenie LiteLLM). W interfejsie nadajesz hubowi **własną nazwę wyświetlaną** (nie musi to być słowo „LiteLLM”). Pozwala przełączać provider bez restartu Claude Code.

## Architektura

- **`src/`** - frontend SolidJS (panel zarządzania, tray icon)
- **`src-tauri/`** - backend Rust (Tauri), bundle do aplikacji `.app`

---

## Wymagania

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Zależności JS
npm install
```

---

## Uruchomienie

### Dev (hot reload)

```bash
npm run tauri dev
```

Uruchamia Vite dev server + aplikację Tauri. Ikona pojawi się w tray barze. Logi Rust widoczne w terminalu.

### Produkcja - kompilacja

```bash
npm run tauri build
open src-tauri/target/release/bundle/macos/ClaudeProxy.app
```

### DMG do dystrybucji

```
src-tauri/target/release/bundle/dmg/ClaudeProxy_*.dmg
```

---

## Konfiguracja Claude Code

Proxy nasłuchuje na porcie **3456**.

**Opcje:**

1. **Z panelu (Status → Install)** — zapisuje `ANTHROPIC_BASE_URL` do `~/.claude/settings.json` (i robi kopię zapasową poprzedniej wartości). Nie musisz edytować shella ręcznie.
2. **Ręcznie w shellu** — np. w `~/.zshrc`:

```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:3456
```

---

## Uwierzytelnianie

### Claude (Anthropic)

Proxy **nie wstawia własnego klucza** — przekazuje dalej nagłówki z Claude Code (`x-api-key` / `Authorization`). Musisz być zalogowany w **Claude Code** (typowo OAuth: `claude auth login`).

**Jak sprawdzić:**

- W terminalu: `claude auth status` — w odpowiedzi JSON pole `loggedIn` (na macOS OAuth jest w Keychain).
- W aplikacji: **Ustawienia** — kolorowa kropka przy statusie logowania; przycisk **„Zaloguj przez CLI”** (macOS: otwiera Terminal z `claude auth login`).

### Hub (OpenAI-compatible / LiteLLM)

Przy wybranym drugim providerze: w **Ustawieniach** ustaw **nazwę w interfejsie** (jak ma się nazywać w UI), **URL** i **klucz API** do Twojego proxy; opcjonalnie **URL panelu zużycia** (link w zakładce Użycie). Mapowanie modeli Claude → modele upstream: zakładka **Modele**.

---

## Logi

### Dev

Logi Rust bezpośrednio w terminalu z `npm run tauri dev`.

### Produkcja

```bash
# Logi na żywo (systemowe):
log stream --predicate 'process == "claude-proxy"' --level debug

# Lub uruchom .app z terminala - logi na stdout (plik wykonywalny ma nazwę z Cargo):
"/Applications/ClaudeProxy.app/Contents/MacOS/claude-proxy"

# Zdarzenia proxy (żądania, statusy upstreamu):
~/.config/claude-proxy/proxy.log
```
