<script lang="ts">
  import { api, errorMessage } from "$lib/api";
  import { PALETTES, applyPalette, savedPalette, type PaletteId } from "$lib/palette";

  let palette = $state<PaletteId>(savedPalette());

  function choose(id: PaletteId) {
    palette = id;
    applyPalette(id);
  }

  let hasKey = $state<boolean | null>(null);
  let key = $state("");
  let message = $state("");
  let error = $state("");

  async function refresh() {
    hasKey = await api.hasApiKey();
  }
  refresh();

  async function save(value: string) {
    message = "";
    error = "";
    try {
      await api.setApiKey(value);
      key = "";
      await refresh();
      message = value ? "Saved to your system keyring." : "Removed.";
    } catch (e) {
      error = errorMessage(e);
    }
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

<section class="card">
  <h2>Anthropic API key</h2>
  <p class="muted">
    Stored in your OS keyring and only read by the app's backend; the UI never sees it.
    {#if hasKey !== null}
      <br />Status: <strong>{hasKey ? "set" : "not set"}</strong>
    {/if}
  </p>
  <form onsubmit={(e) => { e.preventDefault(); save(key); }}>
    <input type="password" bind:value={key} placeholder="sk-ant-…" autocomplete="off" />
    <button class="primary" type="submit" disabled={!key.trim()}>Save</button>
    {#if hasKey}<button type="button" onclick={() => save("")}>Remove</button>{/if}
  </form>
  {#if message}<p>{message}</p>{/if}
  {#if error}<p class="error">{error}</p>{/if}
</section>

<style>
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
