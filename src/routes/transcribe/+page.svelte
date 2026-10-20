<script lang="ts">
  import { goto } from "$app/navigation";
  import { tick } from "svelte";
  import {
    api,
    errorMessage,
    UNSURE_BELOW,
    type Draft,
    type DraftLine,
    type ChatTurn,
    type Suggestion,
    type TranscribeProgress,
  } from "$lib/api";
  import Discussion from "$lib/Discussion.svelte";

  type Row = DraftLine & {
    breakBefore: boolean;
    edited: boolean;
    suggestions: Suggestion[];
    suspicious: string | null;
    discussing: boolean;
    chat: ChatTurn[];
  };

  const STAGE_LABELS: Record<string, string> = {
    metadata: "Look up the video",
    download: "Download audio",
    captions: "Use uploaded captions",
    separate: "Separate vocals",
    transcribe: "Transcribe with Whisper",
  };

  let url = $state("");
  let running = $state(false);
  let error = $state("");
  let stages = $state<TranscribeProgress[]>([]);

  let draft = $state<Draft | null>(null);
  let title = $state("");
  let artist = $state("");
  let rows = $state<Row[]>([]);
  let current = $state(0);
  let editing = $state<number | null>(null);
  let editText = $state("");
  let useVocals = $state(false);
  let audio = $state<HTMLAudioElement>();
  let stopAt = 0;

  /** null until checked; the audio player is only created when playback won't crash the page. */
  let audioOk = $state<boolean | null>(null);
  let audioMissing = $state<string[]>([]);
  api.audioSupport().then((s) => {
    audioOk = s.ok;
    audioMissing = s.missing;
  });

  let proofing = $state(false);
  let proofNote = $state("");
  let saving = $state(false);

  async function start() {
    if (!url.trim() || running) return;
    running = true;
    error = "";
    stages = [];
    draft = null;
    const unlisten = await api.onTranscribeProgress((p) => {
      if (p.stage === "done") return;
      const i = stages.findIndex((s) => s.stage === p.stage);
      if (i === -1) stages.push(p);
      else stages[i] = p;
    });
    try {
      load(await api.transcribeYoutube(url));
    } catch (e) {
      error = errorMessage(e);
    } finally {
      unlisten();
      running = false;
    }
  }

  function load(d: Draft) {
    draft = d;
    title = d.title;
    artist = d.artist;
    rows = d.lines.map((l, i) => ({
      ...l,
      breakBefore: i > 0 && l.stanza !== d.lines[i - 1].stanza,
      edited: false,
      suggestions: [],
      suspicious: null,
      discussing: false,
      chat: [],
    }));
    current = 0;
    proofNote = "";
  }

  /** Rows back to draft lines, with stanzas renumbered and edited lines' words dropped. */
  function toLines(): DraftLine[] {
    let stanza = 0;
    return rows.map((r, i) => {
      if (i > 0 && r.breakBefore) stanza += 1;
      return { text: r.text, start: r.start, end: r.end, stanza, words: r.edited ? [] : r.words, flags: r.flags };
    });
  }

  const audioPath = $derived(draft ? (useVocals && draft.vocals_path ? draft.vocals_path : draft.audio_path) : "");
  let audioSrc = $state("");
  let audioError = $state("");

  // Keep the current track in the player until the next one is ready: an empty src counts as an error.
  $effect(() => {
    const path = audioPath;
    if (!path) return;
    let cancelled = false;
    api.audioUrl(path).then(
      async (url) => {
        if (cancelled) return URL.revokeObjectURL(url);
        const old = audioSrc;
        audioSrc = url;
        await tick();
        if (old) URL.revokeObjectURL(old);
      },
      (e) => {
        if (!cancelled) audioError = `Couldn't load the audio (${e}).`;
      },
    );
    return () => {
      cancelled = true;
    };
  });
  $effect(() => () => {
    if (audioSrc) URL.revokeObjectURL(audioSrc);
  });

  /** What the player reports, for the log and for error messages. */
  function audioState(event: string) {
    const a = audio;
    if (!a) return `${event}: no audio element`;
    const err = a.error ? ` error=${a.error.code}:${a.error.message}` : "";
    return `${event}: readyState=${a.readyState} networkState=${a.networkState} currentTime=${a.currentTime.toFixed(2)} paused=${a.paused} muted=${a.muted} volume=${a.volume}${err} canPlay(wav)=${a.canPlayType("audio/wav")} src=${a.currentSrc || a.src}`;
  }

  function onAudioEvent(e: Event) {
    api.logClient(audioState(e.type));
    if (e.type === "error") audioError = `Couldn't load the audio (${audio?.error?.message || `code ${audio?.error?.code}`}).`;
    if (e.type === "playing" || e.type === "canplay") audioError = "";
  }

  function play(i: number) {
    const row = rows[i];
    if (!audio || !row) return;
    current = i;
    audio.currentTime = Math.max(0, row.start - 0.15);
    stopAt = row.end + 0.25;
    api.logClient(audioState(`play line ${i} at ${row.start.toFixed(2)}`));
    audio.play().catch((e) => {
      audioError = `Couldn't play: ${e.name}: ${e.message}`;
      api.logClient(audioState(`play() rejected ${e.name}: ${e.message}`));
    });
    // If nothing has started a few seconds later, say so instead of failing silently.
    setTimeout(() => {
      if (audio && audio.paused && stopAt && !audioError) {
        audioError = "The audio didn't start. Details are in the app's log.";
        api.logClient(audioState("still paused 3s after play"));
      }
    }, 3000);
  }

  function onTimeUpdate() {
    if (audio && stopAt && audio.currentTime >= stopAt) {
      audio.pause();
      stopAt = 0;
    }
  }

  async function edit(i: number) {
    current = i;
    editing = i;
    editText = rows[i].text;
    await tick();
    document.getElementById(`edit-${i}`)?.focus();
  }

  function commit(i: number) {
    const text = editText.trim();
    if (editing !== i) return;
    editing = null;
    if (text && text !== rows[i].text) setText(i, text);
  }

  function setText(i: number, text: string) {
    const row = rows[i];
    row.text = text;
    row.edited = true;
    row.flags = row.flags.filter((f) => f !== "unsure");
  }

  function accept(i: number, s: Suggestion) {
    const row = rows[i];
    const at = row.text.toLowerCase().indexOf(s.original.toLowerCase());
    if (at !== -1) setText(i, row.text.slice(0, at) + s.replacement + row.text.slice(at + s.original.length));
    dismiss(i, s);
  }

  function dismiss(i: number, s: Suggestion) {
    rows[i].suggestions = rows[i].suggestions.filter((x) => x !== s);
  }

  function remove(i: number) {
    const breakBefore = rows[i].breakBefore;
    rows.splice(i, 1);
    if (rows[i] && breakBefore) rows[i].breakBefore = true;
    current = Math.min(current, rows.length - 1);
  }

  async function proofread() {
    if (proofing) return;
    proofing = true;
    proofNote = "";
    error = "";
    try {
      const result = await api.proofread(title, artist, toLines());
      for (const row of rows) {
        row.suggestions = [];
        row.suspicious = null;
      }
      for (const s of result.suggestions) rows[s.line]?.suggestions.push(s);
      for (const s of result.suspicious_lines) if (rows[s.line]) rows[s.line].suspicious = s.reason;
      const n = result.suggestions.length;
      const flagged = result.suspicious_lines.length;
      proofNote =
        n + flagged === 0
          ? "Claude found nothing to change."
          : `${n} suggestion${n === 1 ? "" : "s"}${flagged ? `, ${flagged} line${flagged === 1 ? "" : "s"} to check` : ""}. Listen before accepting.`;
      if (result.rejected) proofNote += ` (${result.rejected} dropped for not sounding like the original.)`;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      proofing = false;
    }
  }

  async function save() {
    if (!draft || saving) return;
    saving = true;
    error = "";
    const lyrics = rows.map((r, i) => (i > 0 && r.breakBefore ? "\n" : "") + r.text).join("\n");
    try {
      const id = await api.createSong(title, artist, lyrics, `${draft.method}:${draft.video_id}`);
      goto(`/song/${id}`);
    } catch (e) {
      error = errorMessage(e);
      saving = false;
    }
  }

  async function move(delta: number) {
    current = Math.min(rows.length - 1, Math.max(0, current + delta));
    await tick();
    document.getElementById(`row-${current}`)?.scrollIntoView({ block: "nearest" });
  }

  function onKeydown(e: KeyboardEvent) {
    if (!draft || (e.target as HTMLElement).closest("input, textarea") || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === "j" || e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
    } else if (e.key === "k" || e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
    } else if (e.key === "p" || e.key === " ") {
      e.preventDefault();
      play(current);
    } else if (e.key === "Enter") {
      e.preventDefault();
      edit(current);
    }
  }

  function flagLabel(flag: string, row: Row) {
    switch (flag) {
      case "unsure":
        return `${row.words.filter((w) => w.prob < UNSURE_BELOW).length} unsure`;
      case "no-speech":
        return "maybe no singing here";
      case "stock-phrase":
        return "looks like a subtitle credit";
      case "loop":
        return "repeated suspiciously";
      default:
        return flag;
    }
  }

  const unsureCount = $derived(rows.filter((r) => r.flags.length > 0 || r.suspicious).length);
</script>

<svelte:window onkeydown={onKeydown} />

<div class="head">
  <h1>From YouTube</h1>
  <a href="/">← Library</a>
</div>

{#if !draft}
  <section class="card start">
    <form onsubmit={(e) => { e.preventDefault(); start(); }}>
      <input bind:value={url} placeholder="https://www.youtube.com/watch?v=…" disabled={running} />
      <button class="primary" type="submit" disabled={running || !url.trim()}>
        {running ? "Working…" : "Get lyrics"}
      </button>
    </form>
    <p class="muted small">
      Uses the video's uploaded Spanish captions if it has them. Otherwise it separates the vocals and transcribes them on
      this computer with Whisper, which takes a few minutes (the first run also downloads the models).
    </p>
    {#if stages.length}
      <ol class="stages">
        {#each stages as s, i (s.stage)}
          {@const done = i < stages.length - 1 || !running}
          <li class:done>
            <span class="mark">{done ? "✓" : "…"}</span>
            <span>{STAGE_LABELS[s.stage] ?? s.message}</span>
            {#if !done && s.fraction !== null}
              <span class="bar"><span style="width: {Math.round(s.fraction * 100)}%"></span></span>
              <span class="muted small">{Math.round(s.fraction * 100)}%</span>
            {/if}
          </li>
        {/each}
      </ol>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
  </section>
{:else}
  <section class="card meta">
    <label>Title <input bind:value={title} /></label>
    <label>Artist <input bind:value={artist} /></label>
    <div class="actions">
      <span class="muted small">
        {draft.method === "youtube-subs" ? "From the video's uploaded captions" : `Whisper${draft.device ? ` (${draft.device.toUpperCase()})` : ""}`}
        · {rows.length} lines{unsureCount ? ` · ${unsureCount} to check` : ""}
      </span>
      {#if draft.vocals_path}
        <label class="toggle"><input type="checkbox" bind:checked={useVocals} /> Play vocals only</label>
      {/if}
      <button onclick={proofread} disabled={proofing}>{proofing ? "Claude is reading…" : "Check with Claude"}</button>
      <button class="primary" onclick={save} disabled={saving || !title.trim() || rows.length === 0}>
        {saving ? "Saving…" : "Save song"}
      </button>
    </div>
    {#if proofNote}<p class="small">{proofNote}</p>{/if}
    {#if error}<p class="error">{error}</p>{/if}
  </section>

  <p class="muted small keys">
    <kbd>p</kbd> play line · <kbd>j</kbd>/<kbd>k</kbd> move · <kbd>Enter</kbd> edit · underlined words are ones Whisper wasn't sure of
  </p>

  {#if audioOk}
    <audio
      bind:this={audio}
      src={audioSrc}
      ontimeupdate={onTimeUpdate}
      preload="metadata"
      onloadedmetadata={onAudioEvent}
      oncanplay={onAudioEvent}
      onplaying={onAudioEvent}
      onerror={onAudioEvent}
      onstalled={onAudioEvent}
      onsuspend={onAudioEvent}
      onabort={onAudioEvent}
      onemptied={onAudioEvent}
    ></audio>
    {#if audioError}<p class="error small">{audioError}</p>{/if}
  {:else if audioOk === false}
    <p class="card small audio-off">
      Playback is off: your system is missing GStreamer's {audioMissing.join(" and ")}, and the app's web view
      crashes without them. Install them with <code>sudo pacman -S gst-plugins-good</code>, then restart the app.
    </p>
  {/if}

  <ol class="rows">
    {#each rows as row, i (row)}
      <li id="row-{i}" class:current={i === current} class:gap={row.breakBefore}>
        <div class="line">
          <button class="play" onclick={() => play(i)} aria-label="Play line">▶</button>
          {#if editing === i}
            <input
              id="edit-{i}"
              lang="es"
              bind:value={editText}
              onblur={() => commit(i)}
              onkeydown={(e) => {
                if (e.key === "Enter") commit(i);
                if (e.key === "Escape") editing = null;
              }}
            />
          {:else}
            <button class="text" lang="es" onclick={() => edit(i)} title="Click to edit">
              {#if !row.edited && row.words.length}
                {#each row.words as w, wi}<span class:unsure={w.prob < UNSURE_BELOW} title={`${Math.round(w.prob * 100)}% sure`}>{w.text}</span>{wi < row.words.length - 1 ? " " : ""}{/each}
              {:else}
                {row.text}
              {/if}
            </button>
          {/if}
          <span class="tools">
            <button
              class="link"
              onclick={() => (row.discussing = !row.discussing)}
              aria-expanded={row.discussing}
              title="Talk this line over with Claude"
            >
              {row.discussing ? "Hide" : row.chat.length ? "Discuss…" : "Discuss"}
            </button>
            <button class="link" onclick={() => (row.breakBefore = !row.breakBefore)} title="Start a new stanza here" disabled={i === 0}>¶</button>
            <button class="link" onclick={() => remove(i)} title="Delete line">✕</button>
          </span>
        </div>
        {#if row.flags.length || row.suspicious}
          <div class="flags">
            {#each row.flags as f}<span class="tag flag">{flagLabel(f, row)}</span>{/each}
            {#if row.suspicious}<span class="tag flag">Claude: {row.suspicious}</span>{/if}
          </div>
        {/if}
        {#each row.suggestions as s}
          <div class="suggestion">
            <span lang="es"><del>{s.original}</del> → <ins>{s.replacement}</ins></span>
            <span class="muted small">{s.reason}</span>
            <button class="link" onclick={() => play(i)}>Listen</button>
            <button class="link" onclick={() => accept(i, s)}>Use</button>
            <button class="link" onclick={() => dismiss(i, s)}>Dismiss</button>
            {#if !row.discussing}<button class="link" onclick={() => (row.discussing = true)}>Discuss</button>{/if}
          </div>
        {/each}
        {#if row.discussing}
          <Discussion
            {title}
            {artist}
            lines={toLines}
            line={i}
            suggestions={row.suggestions}
            suspicious={row.suspicious}
            bind:messages={row.chat}
          />
        {/if}
      </li>
    {/each}
  </ol>
{/if}

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: 1rem;
  }
  .small {
    font-size: 0.85rem;
  }
  .start {
    max-width: 720px;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .start p {
    margin: 0;
  }
  form {
    display: flex;
    gap: 0.5rem;
  }
  .stages {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .stages li {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  .stages li.done {
    color: var(--muted);
  }
  .mark {
    width: 1.2rem;
    text-align: center;
    color: var(--accent);
  }
  .bar {
    flex: 1;
    max-width: 14rem;
    height: 6px;
    background: var(--surface-2);
    border-radius: 3px;
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .meta {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.6rem 1rem;
    margin-bottom: 0.75rem;
  }
  .meta label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.9rem;
    color: var(--muted);
  }
  .meta label.toggle {
    flex-direction: row;
    align-items: center;
    color: var(--text);
  }
  .meta label.toggle input {
    width: auto;
  }
  .actions {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-wrap: wrap;
  }
  .actions .muted {
    margin-right: auto;
  }
  .meta p {
    grid-column: 1 / -1;
    margin: 0;
  }
  .keys {
    margin: 0 0 0.5rem;
  }
  .audio-off {
    margin: 0 0 0.75rem;
    border-color: var(--accent-2);
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .rows li {
    border-radius: 8px;
    padding: 0.15rem 0.25rem;
    border: 1px solid transparent;
  }
  .rows li.gap {
    margin-top: 1rem;
  }
  .rows li.current {
    border-color: var(--accent);
    background: var(--surface);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .play {
    min-height: 2rem;
    padding: 0 0.6rem;
    font-size: 0.8rem;
    flex: none;
  }
  .text {
    flex: 1;
    text-align: left;
    border: none;
    background: none;
    padding: 0.35rem 0.4rem;
    min-height: 2.2rem;
  }
  .line input {
    flex: 1;
  }
  .unsure {
    text-decoration: underline wavy var(--accent-2);
    text-underline-offset: 4px;
  }
  .tools {
    flex: none;
    opacity: 0.6;
  }
  .flags,
  .suggestion {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
    padding: 0 0 0.35rem 2.9rem;
  }
  .flag {
    border-color: var(--accent-2);
  }
  del {
    color: var(--danger);
  }
  ins {
    color: var(--accent-2);
    text-decoration: none;
    font-weight: 600;
  }
  @media (max-width: 800px) {
    .meta {
      grid-template-columns: 1fr;
    }
    form {
      flex-direction: column;
    }
    .flags,
    .suggestion {
      padding-left: 0.5rem;
    }
  }
</style>
