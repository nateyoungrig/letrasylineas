<script lang="ts">
  import { page } from "$app/state";
  import { tick } from "svelte";
  import { api, errorMessage, type Attempt, type Line, type Song } from "$lib/api";
  import LinePanel from "$lib/LinePanel.svelte";

  let song = $state<Song | null>(null);
  let error = $state("");
  let selectedId = $state<number | null>(null);
  let panel = $state<ReturnType<typeof LinePanel>>();

  $effect(() => {
    const id = Number(page.params.id);
    api
      .getSong(id)
      .then((s) => {
        song = s;
        const firstOpen = s.lines.find((l) => l.repeat_of === null && !l.latest);
        selectedId = (firstOpen ?? s.lines[0])?.id ?? null;
      })
      .catch((e) => (error = errorMessage(e)));
  });

  /** Reloads after an edit, keeping the selection when that line still exists. */
  async function reload() {
    if (!song) return;
    const s = await api.getSong(song.id);
    song = s;
    if (!s.lines.some((l) => l.id === selectedId)) selectedId = s.lines.find((l) => l.repeat_of === null)?.id ?? null;
  }

  // Editing the whole song.
  let editing = $state(false);
  let editTitle = $state("");
  let editArtist = $state("");
  let editLyrics = $state("");
  let editMessage = $state("");
  let savingEdit = $state(false);

  function startEdit() {
    if (!song) return;
    editTitle = song.title;
    editArtist = song.artist;
    editLyrics = song.lines.map((l, i) => (i > 0 && song!.lines[i - 1].stanza !== l.stanza ? "\n" : "") + l.text).join("\n");
    editMessage = "";
    editing = true;
  }

  async function saveEdit() {
    if (!song || savingEdit) return;
    savingEdit = true;
    editMessage = "";
    try {
      const plan = await api.updateSong(song.id, editTitle, editArtist, editLyrics, false);
      if (plan.removed_graded > 0) {
        const n = plan.removed_graded;
        if (!confirm(`This removes ${n} line${n === 1 ? "" : "s"} you've already graded, and those grades go with them. Save anyway?`)) return;
      }
      await api.updateSong(song.id, editTitle, editArtist, editLyrics, true);
      await reload();
      editing = false;
      const parts = [
        plan.changed && `${plan.changed} changed`,
        plan.added && `${plan.added} added`,
        plan.removed && `${plan.removed} removed`,
      ].filter(Boolean);
      editMessage = parts.length ? `Saved: ${parts.join(", ")}.` : "Saved; nothing changed.";
      if (plan.changed) editMessage += " Changed lines keep their grades, marked for regrading.";
    } catch (e) {
      editMessage = errorMessage(e);
    } finally {
      savingEdit = false;
    }
  }

  const originals = $derived(song?.lines.filter((l) => l.repeat_of === null) ?? []);
  const selected = $derived(song?.lines.find((l) => l.id === selectedId) ?? null);
  const graded = $derived(originals.filter((l) => l.latest).length);

  let board = $state<HTMLElement>();
  let side = $state<HTMLElement>();

  /**
   * Scrolls the board just enough to show a line. On narrow windows the panel is a
   * sheet fixed over the board's bottom, so the visible part ends at the sheet's top.
   */
  async function reveal(id: number) {
    await tick();
    const el = document.getElementById(`line-${id}`);
    if (!el || !board) return;
    const margin = 8;
    const area = board.getBoundingClientRect();
    const sheetTop = side && getComputedStyle(side).position === "fixed" ? side.getBoundingClientRect().top : Infinity;
    const top = area.top + margin;
    const bottom = Math.min(area.bottom, sheetTop) - margin;
    const r = el.getBoundingClientRect();
    if (r.top < top) board.scrollTop -= top - r.top;
    else if (r.bottom > bottom) board.scrollTop += r.bottom - bottom;
  }

  function select(line: Line) {
    selectedId = line.repeat_of ?? line.id;
    reveal(line.id);
  }

  function move(delta: number) {
    const i = originals.findIndex((l) => l.id === selectedId);
    const next = originals[Math.min(originals.length - 1, Math.max(0, i + delta))];
    if (!next) return;
    selectedId = next.id;
    reveal(next.id);
  }

  function onKeydown(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    if (target.closest("input, textarea") || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === "j" || e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
    } else if (e.key === "k" || e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
    } else if ((e.key === "Enter" || e.key === "i") && selected) {
      e.preventDefault();
      panel?.focus();
    }
  }

  function onGraded(attempt: Attempt, isNew: boolean) {
    if (!song) return;
    for (const line of song.lines) {
      if ((line.repeat_of ?? line.id) === attempt.line_id) {
        line.latest = attempt;
        if (isNew) line.attempt_count += 1;
      }
    }
  }

  const position = $derived(originals.findIndex((l) => l.id === selectedId));
</script>

<svelte:window onkeydown={onKeydown} />

{#if error}
  <p class="error">{error}</p>
{:else if song}
  <div class="head">
    <div>
      <h1>{song.title}</h1>
      <p class="muted">
        {song.artist}{song.artist ? " · " : ""}{graded}/{originals.length} lines translated
        {#if !editing}· <button class="link inline" onclick={startEdit}>Edit lyrics</button>{/if}
      </p>
      {#if editMessage}<p class="small">{editMessage}</p>{/if}
    </div>
    {#if !editing}
      <p class="muted keys"><kbd>j</kbd>/<kbd>k</kbd> move · <kbd>Enter</kbd> translate · <kbd>Esc</kbd> leave box</p>
    {/if}
  </div>

  {#if editing}
    <section class="card editor">
      <div class="fields">
        <label>Title <input bind:value={editTitle} /></label>
        <label>Artist <input bind:value={editArtist} /></label>
      </div>
      <label class="lyrics">
        Lyrics
        <textarea bind:value={editLyrics} lang="es"></textarea>
      </label>
      <p class="muted small">
        Blank lines separate stanzas. Lines you don't touch keep their grades; lines you change keep theirs too, marked
        for regrading.
      </p>
      <div class="actions">
        <button class="primary" onclick={saveEdit} disabled={savingEdit || !editTitle.trim() || !editLyrics.trim()}>
          {savingEdit ? "Saving…" : "Save changes"}
        </button>
        <button class="link" onclick={() => (editing = false)}>Cancel</button>
      </div>
    </section>
  {:else}

  <div class="layout">
    <ol class="board" bind:this={board}>
      {#each song.lines as line, i (line.id)}
        {@const target = line.repeat_of ?? line.id}
        <li
          id="line-{line.id}"
          class:gap={i > 0 && song.lines[i - 1].stanza !== line.stanza}
          class:repeat={line.repeat_of !== null}
          class:selected={target === selectedId}
        >
          <button class="line" onclick={() => select(line)} lang="es">
            <span class="text">{line.text}</span>
            {#if line.latest}
              <span class="score" data-score={line.latest.grade.score}>{line.latest.grade.score}</span>
            {/if}
          </button>
          {#if line.latest && line.repeat_of === null}
            <div class="hover card" role="tooltip">
              <p><strong>{line.latest.grade.best_translation}</strong></p>
              <p class="muted">You: {line.latest.translation}</p>
            </div>
          {/if}
        </li>
      {/each}
    </ol>

    {#if selected}
      <aside class="card side" bind:this={side}>
        <div class="nav">
          <button onclick={() => move(-1)} disabled={position <= 0} aria-label="Previous line" title="Previous line (k)">←</button>
          <button onclick={() => move(1)} disabled={position >= originals.length - 1} aria-label="Next line" title="Next line (j)">→</button>
          <button onclick={() => (selectedId = null)} aria-label="Close" title="Close">✕</button>
        </div>
        <LinePanel
          bind:this={panel}
          lineId={selected.id}
          text={selected.text}
          attempt={selected.latest}
          source="board"
          onGraded={onGraded}
          onEdited={() => reload()}
        />
      </aside>
    {/if}
  </div>
  {/if}
{:else}
  <p class="muted">Loading…</p>
{/if}

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: end;
    gap: 1rem;
    flex-wrap: wrap;
    margin-bottom: 1rem;
  }
  .inline {
    padding: 0;
    min-height: 0;
    font-size: inherit;
  }
  .small {
    font-size: 0.85rem;
  }
  .editor {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.6rem;
  }
  .editor label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.9rem;
    color: var(--muted);
  }
  .editor .lyrics {
    flex: 1;
    min-height: 0;
  }
  .editor textarea {
    flex: 1;
    min-height: 12rem;
    color: var(--text);
  }
  .editor p {
    margin: 0;
  }
  .actions {
    display: flex;
    gap: 0.75rem;
    align-items: center;
  }
  .head p {
    margin: 0;
  }
  .keys {
    font-size: 0.85rem;
  }
  .head {
    flex: none;
  }
  /* Each pane scrolls on its own and never hands the scroll to the other. */
  .layout {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    gap: 1.5rem;
    align-items: start;
  }
  .board {
    list-style: none;
    margin: 0;
    padding: 0 0 5rem;
    align-self: stretch;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  li {
    position: relative;
  }
  li.gap {
    margin-top: 1rem;
  }
  .line {
    width: 100%;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.75rem;
    text-align: left;
    border: 1px solid transparent;
    background: none;
    padding: 0.4rem 0.6rem;
    border-radius: 8px;
    min-height: 2.5rem;
  }
  .line:hover {
    background: var(--surface-2);
  }
  li.selected .line {
    background: var(--surface);
    border-color: var(--accent);
  }
  li.repeat .text {
    color: var(--muted);
    font-style: italic;
  }
  .line .score {
    flex: none;
  }
  .hover {
    display: none;
    position: absolute;
    left: 1rem;
    right: 1rem;
    top: 100%;
    z-index: 5;
    box-shadow: var(--shadow);
    padding: 0.6rem 0.8rem;
    pointer-events: none;
  }
  .hover p {
    margin: 0;
  }
  @media (hover: hover) {
    li:not(.selected):hover .hover {
      display: block;
    }
  }
  .side {
    max-height: 100%;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .nav {
    display: flex;
    justify-content: flex-end;
    gap: 0.35rem;
    margin: -0.25rem -0.25rem 0.25rem 0;
  }
  .nav button {
    min-width: 2.5rem;
    padding: 0.3rem 0.6rem;
  }

  @media (max-width: 800px) {
    .layout {
      grid-template-columns: 1fr;
    }
    .keys {
      display: none;
    }
    .board {
      /* room to scroll the last lines above the sheet, which can be 65vh tall */
      padding-bottom: 70vh;
    }
    .side {
      position: fixed;
      left: 0;
      right: 0;
      bottom: 0;
      top: auto;
      max-height: 65vh;
      border-radius: var(--radius) var(--radius) 0 0;
      box-shadow: var(--shadow);
      z-index: 20;
    }

  }
</style>
