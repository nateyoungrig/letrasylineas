<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { api, errorMessage, type AiProvider, type AiStatus } from "$lib/api";
  import { PALETTES, applyPalette, savedPalette, type PaletteId } from "$lib/palette";

  let palette = $state<PaletteId>(savedPalette());

  function choose(id: PaletteId) {
    palette = id;
    applyPalette(id);
  }

  const providers: { id: AiProvider; name: string; models: string[] }[] = [
    { id: "anthropic", name: "Anthropic API", models: ["claude-opus-5-5"] },
    { id: "chatgpt", name: "ChatGPT subscription (Codex)", models: ["gpt-6.1-sol", "gpt-6-astra"] },
    { id: "openai", name: "OpenAI API", models: ["gpt-6.1-sol", "gpt-6-astra"] },
    { id: "openrouter", name: "OpenRouter", models: ["openai/gpt-6.1-sol"] },
  ];
  let status = $state<AiStatus | null>(null);
  let provider = $state<AiProvider>("anthropic");
  let model = $state("");
  let key = $state("");
  let message = $state("");
  let error = $state("");
  let busy = $state(false);
  let signingIn = $state(false);
  const selected = $derived(providers.find((p) => p.id === provider)!);
  const connection = $derived(status?.connections.find((p) => p.provider === provider));
  const active = $derived(providers.find((p) => p.id === status?.settings.provider));

  async function refresh(initial = false) {
    status = await api.aiStatus();
    if (initial) provider = status.settings.provider;
    if (initial) selectProvider();
  }
  onMount(() => { refresh(true).catch((e) => { error = errorMessage(e); }); });

  onDestroy(() => { if (signingIn) api.cancelAiLogin().catch(() => {}); });

  function selectProvider() {
    model = status?.settings.models[provider] ?? selected.models[0];
    key = "";
    message = "";
    error = "";
  }

  async function action(work: () => Promise<void>, success: string) {
    busy = true;
    message = "";
    error = "";
    try {
      await work();
      await refresh();
      message = success;
    } catch (e) {
      error = errorMessage(e);
      await refresh().catch(() => {});
    } finally {
      busy = false;
      signingIn = false;
    }
  }

  async function saveKey() {
    const value = key;
    key = "";
    await action(() => api.setProviderKey(provider, value), "Saved to your system keyring.");
  }

  async function login() {
    signingIn = true;
    await action(() => api.loginProvider(provider), "Connected. Choose Use this provider to make it active.");
  }
</script>

<h1>Settings</h1>

<section class="card">
  <h2>Colour scheme</h2>
  <p class="muted">Follows your system's light or dark mode.</p>
  <div class="palettes">
    {#each PALETTES as p (p.id)}
      <button
        class="palette"
        data-palette={p.id}
        aria-pressed={palette === p.id}
        onclick={() => choose(p.id)}
      >
        <span class="swatches">
          <span style="background: var(--accent)"></span>
          <span style="background: var(--accent-2)"></span>
          <span style="background: var(--surface-2)"></span>
        </span>
        <strong>{p.name}</strong>
        <span class="blurb">{p.blurb}</span>
        <span class="sample"><span class="score" data-score="5">5</span><span class="pill">Submit</span></span>
      </button>
    {/each}
  </div>
</section>

<section class="card" aria-busy={busy}>
  <h2>Your AI</h2>
  {#if status && active}
    <p>Active: <strong>{active.name}</strong> · {status.settings.models[status.settings.provider]}</p>
  {:else}
    <p class="muted">Loading AI settings…</p>
  {/if}
  <label for="provider">Provider</label>
  <select id="provider" bind:value={provider} onchange={selectProvider} disabled={busy || !status}>
    {#each providers as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
  </select>

  {#if provider === "chatgpt"}
    <p class="muted">
      Uses your installed Codex CLI and its ChatGPT sign-in. Usage counts toward your plan's Codex allowance.
      Install Codex CLI 0.167 or newer if needed. The app never reads Codex tokens.
    </p>
    <p class="muted">Sign-in is shared with Codex CLI. Signing out here signs the CLI out too. New sign-ins use the OS keyring.</p>
  {:else}
    <p class="muted">
      {provider === "openrouter" ? "Connect your OpenRouter account or enter a key. Usage uses your OpenRouter credits." : "Usage is billed to your API account."}
      Keys are stored in your OS keyring. Saved credentials are never sent back to this page.
    </p>
  {/if}
  {#if connection}
    <p>Status: <strong>{connection.connected ? "connected" : "not connected"}</strong>{connection.source === "environment" ? " (environment variable)" : ""}</p>
    {#if connection.error}<p class="error">{connection.error}</p>{/if}
    {#if connection.source === "environment"}
      <p class="muted">To disconnect this environment key, remove the provider's environment variable and restart the app.</p>
    {/if}
  {/if}

  <div class="actions">
    {#if provider === "chatgpt" || provider === "openrouter"}
      <button onclick={login} disabled={busy || !status}>{signingIn ? "Waiting for browser…" : provider === "chatgpt" ? "Sign in with ChatGPT" : "Connect OpenRouter"}</button>
    {/if}
    {#if connection?.connected && connection.source !== "environment"}
      <button disabled={busy} onclick={() => action(() => api.logoutProvider(provider), provider === "chatgpt" ? "Signed out of Codex." : "Saved key removed. Revoke it at your provider to disable it elsewhere.")}>
        {provider === "chatgpt" ? "Sign out of Codex" : "Remove saved key"}
      </button>
    {/if}
    {#if signingIn}
      <button onclick={() => api.cancelAiLogin().catch((e) => { error = errorMessage(e); })}>Cancel sign-in</button>
    {/if}
    <button disabled={busy} onclick={() => action(() => refresh(), "Connection status refreshed.")}>Refresh status</button>
  </div>

  {#if provider !== "chatgpt"}
    <form onsubmit={(e) => { e.preventDefault(); saveKey(); }}>
      <label class="sr-only" for="api-key">API key</label>
      <input id="api-key" type="password" bind:value={key} placeholder="API key" autocomplete="off" disabled={busy || !status} />
      <button type="submit" disabled={busy || !key.trim()}>Save key</button>
    </form>
  {/if}

  <form class="model-form" onsubmit={(e) => { e.preventDefault(); action(() => api.saveAiSettings(provider, model), "Active provider and model saved."); }}>
    <label for="model">Model</label>
    <input id="model" list="models" bind:value={model} disabled={busy || !status} required autocomplete="off" />
    <datalist id="models">{#each selected.models as value}<option {value}></option>{/each}</datalist>
    <p class="muted">Choose a suggested model or enter a model ID available to your account. It must support reasoning and structured output.</p>
    <button class="primary" type="submit" disabled={busy || !model.trim() || !status}>Use this provider</button>
  </form>
  <div aria-live="polite">
    {#if message}<p>{message}</p>{/if}
    {#if error}<p class="error">{error}</p>{/if}
  </div>
</section>

<style>
  .actions { display: flex; flex-wrap: wrap; gap: 0.5rem; margin-bottom: 1rem; }
  .model-form { display: flex; flex-direction: column; margin-top: 1.25rem; }
  #provider { display: block; width: 100%; margin-top: 0.4rem; }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
  section {
    max-width: 640px;
    margin-bottom: 1rem;
  }
  .palettes {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 0.6rem;
  }
  .palette {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.3rem;
    text-align: left;
    padding: 0.75rem;
    background: var(--bg);
    color: var(--text);
    border: 2px solid var(--border);
  }
  .palette[aria-pressed="true"] {
    border-color: var(--accent);
  }
  .swatches {
    display: flex;
    gap: 0.25rem;
  }
  .swatches span {
    width: 1.4rem;
    height: 1.4rem;
    border-radius: 999px;
    border: 1px solid var(--border);
  }
  .blurb {
    font-size: 0.8rem;
    color: var(--muted);
  }
  .sample {
    display: flex;
    gap: 0.4rem;
    align-items: center;
    margin-top: 0.2rem;
  }
  .pill {
    background: var(--accent);
    color: var(--accent-text);
    font-size: 0.8rem;
    font-weight: 600;
    padding: 0.15rem 0.55rem;
    border-radius: 6px;
  }
  form {
    display: flex;
    gap: 0.5rem;
  }
  @media (max-width: 500px) {
    form {
      flex-direction: column;
    }
  }
</style>
