<script lang="ts">
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import { api, type Attempt, type ReviewItem } from "$lib/api";
  import LinePanel from "$lib/LinePanel.svelte";
  import Tutor from "$lib/Tutor.svelte";

  let queue = $state<ReviewItem[] | null>(null);
  let index = $state(0);
  /** The attempt made in this session; the old grade stays hidden until you answer. */
  let result = $state<Attempt | null>(null);
  let done = $state<number[]>([]);
  let panel = $state<ReturnType<typeof LinePanel>>();

  const tag = $derived(page.url.searchParams.get("tag"));
  const tab = $derived(page.url.searchParams.get("tab") === "tutor" ? "tutor" : "review");
  const current = $derived(queue?.[index] ?? null);

  $effect(() => {
    const t = tag;
    queue = null;
    index = 0;
    result = null;
    done = [];
    api.reviewQueue(t).then((q) => (queue = q));
  });

  $effect(() => {
    current;
    setTimeout(() => panel?.focus());
  });

  function next() {
    result = null;
    index += 1;
  }

  function onGraded(a: Attempt, isNew: boolean) {
    result = a;
    done = isNew ? [...done, a.grade.score] : [...done.slice(0, -1), a.grade.score];
  }

  function onKeydown(e: KeyboardEvent) {
    if (tab === "review" && result && e.key === "Enter" && !(e.target as HTMLElement).closest("input, textarea")) {
      e.preventDefault();
      next();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="head">
  <h1>Repaso</h1>
  <nav class="tabs" aria-label="Repaso">
    <a href={tag ? `/repaso?tag=${tag}` : "/repaso"} aria-current={tab === "review" ? "page" : undefined}>Review</a>
    <a href="/repaso?tab=tutor" aria-current={tab === "tutor" ? "page" : undefined}>Tutor</a>
  </nav>
</div>

{#if tab === "tutor"}
  <Tutor />
{:else}
{#if tag}
  <p class="sub">Practising <span class="tag">{tag}</span> <button class="link" onclick={() => goto("/repaso")}>all skills</button></p>
{:else}
  <p class="sub muted">Lines due for review, weakest first.</p>
{/if}

{#if queue === null}
  <p class="muted">Loading…</p>
{:else if current}
  <p class="muted progress">{index + 1} of {queue.length} · {current.song_title}{current.artist ? ` — ${current.artist}` : ""}</p>
  <div class="card">
    {#key current.line.id}
      <LinePanel
        bind:this={panel}
        lineId={current.line.id}
        text={current.line.text}
        attempt={result}
        source="repaso"
        note={current.line.latest ? `Last time: ${current.line.latest.grade.score}/5` : undefined}
        {onGraded}
        onEdited={(t) => {
          if (current) current.line.text = t;
        }}
      />
    {/key}
  </div>
  {#if result}
    <button class="primary next" onclick={next}>Next <kbd>Enter</kbd></button>
  {/if}
{:else}
  <div class="card">
    {#if done.length}
      <p>
        Done: {done.length} lines, average {(done.reduce((a, b) => a + b, 0) / done.length).toFixed(1)}.
      </p>
    {:else}
      <p>Nothing due{tag ? ` for ${tag}` : ""}. Translate more lines on the board and they'll come back here.</p>
    {/if}
    <a href="/">Back to the library</a>
  </div>
{/if}
{/if}

<style>
  .head {
    display: flex;
    align-items: baseline;
    gap: 1.25rem;
    flex-wrap: wrap;
    margin-bottom: 1rem;
  }
  .head h1 {
    margin: 0;
  }
  .tabs {
    display: flex;
    gap: 0.25rem;
  }
  .tabs a {
    text-decoration: none;
    padding: 0.35rem 0.8rem;
    border-radius: 8px;
    color: var(--muted);
  }
  .tabs a[aria-current="page"] {
    background: var(--surface-2);
    color: var(--text);
    font-weight: 600;
  }
  .sub {
    margin: 0 0 1rem;
  }
  .progress {
    font-size: 0.9rem;
  }
  .card {
    max-width: 720px;
  }
  .next {
    margin-top: 1rem;
  }
</style>
