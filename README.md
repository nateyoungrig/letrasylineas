# Letras y Líneas

Translate Spanish song lyrics line by line; Claude grades each line (1–5) and you can ask follow-ups.

## Run

```sh
npm install
npm run tauri dev
```

Add your Anthropic API key under Settings (stored in the OS keyring), or export `ANTHROPIC_API_KEY`.

## Layout

- `crates/letras-core` — game logic with no Tauri dependency: SQLite storage, local users, Claude client, LRCLIB lookup, Repaso scheduling.
- `src-tauri` — thin Tauri commands over the core crate; holds the API key.
- `src` — SvelteKit UI. `src/lib/api.ts` is the only file that talks to the backend.

## Lyrics lookup

"Find lyrics" queries [LRCLIB](https://lrclib.net) and copies its stored text verbatim. Lyrics never pass through a language model, so a lookup either returns real lyrics or nothing; paste them yourself when LRCLIB lacks a song.

## Tests

```sh
cargo test -p letras-core                  # offline
cargo test -p letras-core -- --ignored     # also hits the live LRCLIB API
```
