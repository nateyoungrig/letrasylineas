# Letras y Líneas

Translate Spanish song lyrics line by line; your chosen AI grades each line (1–5), answers follow-ups, and helps with review.

## Run

```sh
npm install
npm run tauri dev
```

Choose an AI provider and model in **Settings → Your AI**, then click **Use this provider**:

- **Anthropic API**: enter an API key. Existing keys and the original model/settings continue to work.
- **ChatGPT subscription (Codex)**: install Codex CLI 0.167 or newer and sign in with ChatGPT. An existing CLI login works immediately; otherwise use **Sign in with ChatGPT**. Select `gpt-6.1-sol` or another model your account can access. Usage counts against the account's Codex allowance.
- **OpenAI API**: enter an API key for separately billed Responses API usage.
- **OpenRouter**: connect your account in the browser with PKCE, or enter a key. Uses your OpenRouter credits. Choose a model supporting reasoning and strict structured output.

API keys are stored in the OS keyring under `letrasylineas`. Development fallbacks are `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, and `OPENROUTER_API_KEY`; Settings identifies environment credentials because removing a saved key cannot unset an environment variable. Secrets are never returned to the webview.

Codex owns its subscription credentials. New sign-ins from Settings require its OS-keyring storage; existing CLI credentials are reused through Codex without the app reading `auth.json`. Sign-in and sign-out are shared with the installed CLI. See [provider integration notes](docs/ai-providers.md) for authentication decisions, limitations, terms references, and verification.

Provider and per-provider model preferences are saved separately from credentials, in `ai-settings.json` in the app data directory. They are shared across local learner profiles, like the original API key.

## Layout

- `crates/letras-core` — UI-independent logic: SQLite storage, local users, provider-independent AI operations, provider transports, LRCLIB lookup, Repaso scheduling.
- `src-tauri` — Tauri commands, OS-keyring access, account connection flows.
- `src` — SvelteKit UI. `src/lib/api.ts` is the only file that talks to the backend.

## Lyrics sources

Lyrics come only from LRCLIB, YouTube captions, Whisper, or your pasted text. "Find lyrics" copies LRCLIB text verbatim. No AI provider retrieves or generates source lyrics. AI proofreading only proposes screened sound-alike corrections to an existing transcript for the learner to review.

## Checks

```sh
cargo test -p letras-core             # offline tests; live tests are ignored
cargo test -p letrasylineas --lib      # PKCE and callback validation
cargo check
npm run check
```

Opt-in subscription smoke test (five short requests; uses Codex quota):

```sh
cargo test -p letras-core --test codex_live -- --ignored --nocapture
```

Other ignored tests include live LRCLIB/transcription tests and an existing local-library upgrade test. Run those individually when appropriate.
