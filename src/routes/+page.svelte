<script lang="ts">
  import { goto } from "$app/navigation";
  import { api, errorMessage, type LookupResult, type LyricsMatch, type SongSummary, type Stats } from "$lib/api";

  let songs = $state<SongSummary[]>([]);
  let stats = $state<Stats | null>(null);

  let title = $state("");
  let artist = $state("");
  let lyrics = $state("");
  let lrclibId = $state<number | null>(null);
  let saving = $state(false);
  let formError = $state("");

  let lookup = $state<LookupResult | null>(null);
  const matches = $derived(lookup?.matches ?? null);
  let searching = $state(false);
  let searchError = $state("");

  async function load() {
    [songs, stats] = await Promise.all([api.listSongs(), api.getStats()]);
  }
  load();

  async function search() {
    if (!title.trim() || searching) return;
    searching = true;
    searchError = "";
    lookup = null;
    try {
      lookup = await api.lookupLyrics(title, artist);
    } catch (e) {
      searchError = errorMessage(e);
    } finally {
      searching = false;
    }
  }

  function useMatch(m: LyricsMatch) {
    title = m.track;
    artist = m.artist;
    lyrics = m.lyrics;
    lrclibId = m.id;
    lookup = null;
  }

  function onLyricsInput() {
    // Hand edits are fine, but the text is no longer a verbatim LRCLIB copy.
    lrclibId = null;
  }

  async function save() {
    saving = true;
    formError = "";
    try {
      const id = await api.createSong(title, artist, lyrics, lrclibId ? `lrclib:${lrclibId}` : null);
      goto(`/song/${id}`);
    } catch (e) {
      formError = errorMessage(e);
      saving = false;
    }
  }

  async function remove(s: SongSummary) {
    if (!confirm(`Delete “${s.title}” and all its grades?`)) return;
    await api.deleteSong(s.id);
    load();
  }

  function duration(secs: number) {
    const s = Math.round(secs);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }

  function preview(text: string) {
    return text.split("\n").filter((l) => l.trim()).slice(0, 3).join(" / ");
  }
</script>

{#if stats && stats.lines_graded > 0}
  <section class="stats">
    <div class="card stat"><span class="num">{stats.lines_graded}</span><span class="muted">lines translated</span></div>
    <div class="card stat">
      <span class="num">{stats.avg_score?.toFixed(1) ?? "–"}</span><span class="muted">average score</span>
    </div>
    <a class="card stat due" href="/repaso">
      <span class="num">{stats.due_count}</span><span class="muted">due for Repaso</span>
    </a>
    <div class="card dist">
      <span class="muted">Latest scores</span>
      <div class="bars">
        {#each stats.score_counts as count, i}
          {@const max = Math.max(1, ...stats.score_counts)}
          <div class="bar" title="{count} lines scored {i + 1}">
            <div class="fill score" data-score={i + 1} style="height: {(count / max) * 100}%"></div>
            <span>{i + 1}</span>
          </div>
        {/each}
      </div>
    </div>
    {#if stats.tags.length}
      <div class="card weak">
        <span class="muted">Weakest skills</span>
        <div class="tags">
          {#each stats.tags.slice(0, 6) as t}
            <a class="tag" href="/repaso?tag={t.tag}" title="{t.count} lines">{t.tag} · {t.avg_score.toFixed(1)}</a>
          {/each}
        </div>
      </div>
    {/if}
  </section>
{/if}

<div class="cols">
  <section>
    <h2>Songs</h2>
    {#if songs.length === 0}
      <p class="muted">No songs yet. Look one up or paste lyrics to start.</p>
    {/if}
    <ul class="songs">
      {#each songs as s (s.id)}
        <li class="card">
          <a href="/song/{s.id}" class="song">
            <strong>{s.title}</strong>
            <span class="muted">{s.artist}</span>
            <div class="progress" aria-label="{s.graded_count} of {s.line_count} lines">
              <div style="width: {(s.graded_count / s.line_count) * 100}%"></div>
            </div>
            <span class="muted small">
              {s.graded_count}/{s.line_count} lines{s.avg_score !== null ? ` · avg ${s.avg_score.toFixed(1)}` : ""}
            </span>
          </a>
          <button class="link" onclick={() => remove(s)}>Delete</button>
        </li>
      {/each}
    </ul>
  </section>

  <section class="card new">
    <div class="new-head">
      <h2>New song</h2>
      <a href="/transcribe">From a YouTube video →</a>
    </div>
    <form onsubmit={(e) => { e.preventDefault(); search(); }}>
      <label>Title <input bind:value={title} placeholder="Despacito" /></label>
      <label>Artist <input bind:value={artist} placeholder="Luis Fonsi" /></label>
      <button type="submit" disabled={searching || !title.trim()}>{searching ? "Searching…" : "Find lyrics"}</button>
    </form>

    {#if searchError}<p class="error">{searchError}</p>{/if}
    {#if matches}
      {#if matches.length === 0}
        <p class="muted">LRCLIB has no lyrics for that. Check the spelling, or paste them below.</p>
      {:else}
        <p class="muted small">
          {#if lookup && !lookup.artist_matched}
            Nothing by “{artist.trim()}” under that title; here's everything with that title.
          {/if}
          From LRCLIB. Pick the version that matches your recording.
        </p>
        <ul class="matches">
          {#each matches as m (m.id)}
            <li>
              <button class="match" onclick={() => useMatch(m)}>
                <span><strong>{m.track}</strong> · {m.artist}</span>
                <span class="muted small">{m.album || "Unknown album"} · {duration(m.duration_secs)}</span>
                <span class="muted small preview" lang="es">{preview(m.lyrics)}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}

    <label>
      Lyrics
      <textarea bind:value={lyrics} oninput={onLyricsInput} rows="12" lang="es" placeholder="Paste lyrics, or use Find lyrics above"></textarea>
    </label>
    <p class="muted small">
      {lrclibId ? `Verbatim from LRCLIB (record ${lrclibId}).` : "Blank lines separate stanzas; [Coro]-style headers are skipped."}
    </p>
    {#if formError}<p class="error">{formError}</p>{/if}
    <button class="primary" onclick={save} disabled={saving || !title.trim() || !lyrics.trim()}>
      {saving ? "Saving…" : "Start translating"}
    </button>
  </section>
</div>

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 0.75rem;
    margin-bottom: 1.5rem;
  }
  .stat {
    display: flex;
    flex-direction: column;
    text-decoration: none;
  }
  .num {
    font-size: 1.8rem;
    font-weight: 800;
  }
  .due .num {
    color: var(--accent-2);
  }
  .dist,
  .weak {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .weak {
    grid-column: span 2;
  }
  .bars {
    display: flex;
    gap: 0.4rem;
    align-items: end;
    height: 4rem;
  }
  .bar {
    flex: 1;
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: end;
    align-items: center;
    font-size: 0.75rem;
  }
  .fill {
    width: 100%;
    min-height: 2px;
    border-radius: 3px;
    display: block;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }
  .cols {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 1.5rem;
    align-items: start;
  }
  .songs,
  .matches {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .songs li {
    display: flex;
    align-items: start;
    gap: 0.5rem;
  }
  .song {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    text-decoration: none;
    min-width: 0;
  }
  .progress {
    height: 6px;
    background: var(--surface-2);
    border-radius: 3px;
    overflow: hidden;
    margin-top: 0.3rem;
  }
  .progress div {
    height: 100%;
    background: var(--accent);
  }
  .small {
    font-size: 0.85rem;
  }
  .new {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  form {
    display: grid;
    grid-template-columns: 1fr 1fr auto;
    gap: 0.5rem;
    align-items: end;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.9rem;
    color: var(--muted);
  }
  .match {
    width: 100%;
    text-align: left;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .preview {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .new-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 1rem;
    flex-wrap: wrap;
  }
  .new-head h2 {
    margin: 0;
  }
  .new > p {
    margin: 0;
  }
  .new > .primary {
    align-self: flex-start;
  }
  @media (max-width: 800px) {
    .cols {
      grid-template-columns: 1fr;
    }
    .new {
      order: -1;
    }
    form {
      grid-template-columns: 1fr;
    }
    .weak {
      grid-column: auto;
    }
  }
</style>
