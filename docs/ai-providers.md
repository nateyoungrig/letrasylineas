# AI provider integration

Implemented and researched on 2026-11-10.

## Behavior and architecture

`Ai` supplies the five existing operations: grade, follow-up, tutor, proofread, and discuss. Prompts, JSON deserialization, grade normalization, spoiler hiding, and proofreading screening are shared across providers. Database retry logic remains in `service.rs`. `Provider` selects the transport; model IDs are validated and saved per provider in non-secret, atomically replaced settings. Anthropic remains the default for existing installations.

| Provider | Transport | Structured results | Reasoning |
| --- | --- | --- | --- |
| Anthropic | Messages API | Original JSON schema output config | Original adaptive thinking, medium/low effort, 16,000-token limit, server-side fallback beta |
| OpenAI API | Responses API | `text.format`, `json_schema`, `strict: true` | `reasoning.effort`: medium for grade/tutor/proofread, low for follow-up/discuss |
| ChatGPT | Installed, unmodified `codex exec` | `--output-schema` for grade/proofread | Same medium/low mapping through Codex configuration |
| OpenRouter | Chat Completions API | Strict `response_format.json_schema`; routing requires parameter support | Same medium/low mapping through `reasoning.effort` |

OpenAI requests disable response storage. HTTP adapters handle refusals, incomplete/empty responses, authentication errors, and usage limits; raw upstream error bodies are not returned to the UI. Arbitrary model IDs are supported, but models must implement the requested reasoning and schema parameters. Unsupported models fail visibly instead of silently dropping those settings. See [OpenAI structured output documentation](https://developers.openai.com/api/docs/guides/structured-outputs), [OpenRouter structured outputs](https://openrouter.ai/docs/guides/features/structured-outputs), and [OpenRouter reasoning](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens).

No new lyrics source or generation operation exists. LRCLIB lookup, captions, Whisper transcription, and pasted text keep their existing provenance rules.

## Why the installed Codex CLI

OpenAI documents [`codex exec`](https://learn.chatgpt.com/docs/non-interactive-mode) for non-interactive automation, saved CLI authentication, and schema-constrained output. It also documents [app-server](https://learn.chatgpt.com/docs/app-server) for deeper integrations in other products. This app needs short, independent text turns, so `exec` is the smaller supported integration surface. Authentication and token refresh stay inside OpenAI's unmodified CLI. We do not borrow a public OAuth client ID, implement private ChatGPT endpoints, scrape browser sessions, or parse/copy `~/.codex/auth.json`.

The adapter uses a fresh temporary working directory, stdin for prompts, ephemeral sessions, read-only sandboxing, no approvals, and no user/project configuration. Shell, apps, plugins, browser, image, hooks, and multi-agent features are disabled. It extracts the final agent message from completed JSONL events and ignores reasoning. Each request has a three-minute timeout; dropping the operation terminates its own child, never unrelated processes. The installed version tested was `codex-cli 0.167.0`; older versions may lack required isolation flags.

Inherited OpenAI API credentials and base-URL overrides are removed from the child environment; requests require a ChatGPT-authenticated CLI, enforce the ChatGPT login method, and use the built-in OpenAI provider. This avoids accidentally charging an API key when the learner selected subscription usage. The CLI must be available on the desktop app's `PATH`.

## Credentials and account controls

API keys are written only to the OS keyring, service `letrasylineas`, with distinct account names. `anthropic-api-key` is unchanged. OpenRouter's documented [S256 PKCE flow](https://openrouter.ai/docs/guides/overview/auth/oauth) uses random verifier/state, a temporary IPv4 loopback listener on an OS-assigned port, exact state matching, single-use exchange, and a three-minute deadline. Authorization codes and returned keys never enter the webview. The listener never uses port 1420. Sign-in can be cancelled; authentication mutations are serialized.

[Codex credential storage](https://learn.chatgpt.com/docs/auth) is owned by Codex, not this app. Browser sign-in launched from Settings explicitly requires `cli_auth_credentials_store="keyring"`; it fails if the keyring is unavailable. Existing CLI authentication is reused using Codex's `auto` mode, which may read a pre-existing CLI-managed plaintext credential file. **This is a deliberate compatibility exception for an existing CLI login, not a migration to keyring:** the app never opens or copies that file. Users who want all existing Codex credentials moved to the keyring should sign in again using the Settings button. Codex controls refresh/storage of its own credentials.

Signing out of ChatGPT here signs out the shared Codex CLI. Removing an API key deletes the local saved credential; it does not revoke the remote key. Environment fallback credentials must be removed from the app's environment. Connection status means a saved credential or CLI login is present, not that API credit/model entitlement has been live-validated. Settings stores only provider/model preferences and never secrets.

## Terms and scope

The chosen route follows documented local Codex automation, using the individual's own account and plan allowance. It is not a general ChatGPT-subscription API, unlimited usage, resale, account pooling, or a hosted inference service. The [OpenAI Terms of Use](https://openai.com/policies/terms-of-use/) prohibit account sharing, bypassing limits, and certain automated extraction; the documented CLI automation route is the basis for this implementation, not a blanket legal approval for every third-party subscription use. Availability, plan limits, and applicable policies still govern. A distributed commercial/hosted version should confirm its intended use with OpenAI rather than repurpose subscription tokens.

Anthropic's [current authentication and credential-use rules](https://code.claude.com/docs/en/legal-and-compliance#authentication-and-credential-use) still explicitly prohibit third-party Claude.ai login and routing requests through consumer subscription credentials. That restriction has not been removed. The page now also describes conditions for hosting an unmodified Claude Code binary with the user's own authentication; this does not authorize extracting OAuth tokens or offering a native Claude.ai login for this tutor. No Claude.ai subscription integration was implemented.

## Verification and remaining limits

- Existing offline tests pass, including database behavior, spoilers, and proofreading screens. Added tests cover each transport's request/response mapping, reasoning/schema choices, error handling, Codex JSONL extraction and invocation settings, and preference persistence/validation.
- Tauri PKCE tests cover the RFC 7636 S256 vector and matching/duplicate/missing callback parameters.
- `cargo check` and `npm run check` pass.
- Live: all five operations completed using this machine's existing ChatGPT-authenticated Codex CLI and `gpt-6.1-sol`. The smoke test uses ordinary invented Spanish prose, not song lyrics. No API key was used for that test.
- Not live-tested: Anthropic/OpenAI/OpenRouter paid inference, fresh interactive ChatGPT sign-in/sign-out, OpenRouter browser consent/key exchange, or the rendered desktop UI. These require provider credentials or user browser interaction; the existing Codex login was left intact. New HTTP paths and auth mapping have offline coverage, not an assertion of live provider acceptance.
- Model suggestions are editable, not a live account-entitlement catalog. No device-code UI, custom ChatGPT OAuth client, or app-server session manager is included; the installed CLI's browser login/exec route is the implemented mechanism.

The owner's development server and port 1420 were left alone. No push or deployment is part of this work.
