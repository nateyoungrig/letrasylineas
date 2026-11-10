<script lang="ts">
  import { tick } from "svelte";
  import { api, errorMessage, type Attempt, type ThreadMessage } from "$lib/api";

  interface Props {
    /** The line to grade (an original line, never a repeat). */
    lineId: number;
    text: string;
    attempt: Attempt | null;
    source: "board" | "repaso";
    note?: string;
    /** `isNew` is false when an existing attempt was regraded in place. */
    onGraded: (attempt: Attempt, isNew: boolean) => void;
    /** When given, the line can be edited here; called after a successful edit. */
    onEdited?: (text: string) => void;
  }

  let { lineId, text, attempt, source, note, onGraded, onEdited }: Props = $props();

  let editingLine = $state(false);
  let lineDraft = $state("");
  let lineInput = $state<HTMLInputElement>();

  async function startLineEdit() {
    lineDraft = text;
    editingLine = true;
    await tick();
    lineInput?.focus();
  }

  async function saveLine() {
    // Enter saves and then closes the box, which also fires blur; only save once.
    if (!editingLine) return;
    const next = lineDraft.trim();
    editingLine = false;
    if (!next || next === text || !onEdited) return;
    try {
      await api.editLine(lineId, next);
      onEdited(next);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  let draft = $state("");
  let retrying = $state(false);
  let busy = $state(false);
  let error = $state("");
  let thread = $state<ThreadMessage[]>([]);
  let question = $state("");
  let asking = $state(false);
  let input = $state<HTMLTextAreaElement>();

  // Reset when the selected line changes.
  $effect(() => {
    lineId;
    draft = "";
    retrying = false;
    error = "";
    question = "";
  });

  $effect(() => {
    const id = attempt?.id;
    thread = [];
    if (id) api.getThread(id).then((t) => (thread = t));
  });

  const showInput = $derived(!attempt || retrying);

  export function focus() {
    input?.focus();
  }

  async function submit() {
    if (!draft.trim() || busy) return;
    busy = true;
    error = "";
    try {
      const result = await api.submitTranslation(lineId, draft, source);
      retrying = false;
      draft = "";
      onGraded(result, true);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  let regrading = $state(false);

  async function regrade() {
    if (!attempt || regrading) return;
    regrading = true;
    error = "";
    try {
      onGraded(await api.regradeAttempt(attempt.id), false);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      regrading = false;
    }
  }

  let pending = $state<string | null>(null);
  let threadEl = $state<HTMLElement>();

  /** Brings the newest message into view in whichever pane scrolls this panel. */
  async function revealLast() {
    await tick();
    const last = threadEl?.querySelector(".msg:last-of-type");
    last?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  async function ask() {
    if (!attempt || !question.trim() || asking) return;
    asking = true;
    error = "";
    pending = question.trim();
    question = "";
    revealLast();
    try {
      thread = [...thread, ...(await api.askFollowup(attempt.id, pending))];
      pending = null;
      revealLast();
    } catch (e) {
      question = pending ?? "";
      pending = null;
      error = errorMessage(e);
    } finally {
      asking = false;
    }
  }

  function onKey(e: KeyboardEvent, action: () => void) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      action();
    } else if (e.key === "Escape") {
      (e.target as HTMLElement).blur();
    }
  }
</script>

<section class="panel">
  {#if editingLine}
    <input
      class="spanish-edit"
      lang="es"
      bind:this={lineInput}
      bind:value={lineDraft}
      onkeydown={(e) => {
        if (e.key === "Enter") saveLine();
        if (e.key === "Escape") editingLine = false;
      }}
      onblur={saveLine}
    />
    <p class="muted small">Enter to save, Esc to cancel. Repeats of this line change too.</p>
  {:else}
    <div class="spanish-row">
      <p class="spanish" lang="es">{text}</p>
      {#if onEdited}
        <button class="link edit-line" onclick={startLineEdit} title="Fix this line's Spanish" aria-label="Edit line">✎</button>
      {/if}
    </div>
  {/if}
  {#if note}<p class="muted small">{note}</p>{/if}

  {#if showInput}
    <textarea
      bind:this={input}
      bind:value={draft}
      rows="2"
      placeholder="Your English translation…"
      onkeydown={(e) => onKey(e, submit)}
      disabled={busy}
    ></textarea>
    <div class="row">
      <button class="primary" onclick={submit} disabled={busy || !draft.trim()}>
        {busy ? "Grading…" : "Submit"}
      </button>
      {#if retrying}<button class="link" onclick={() => (retrying = false)}>Cancel</button>{/if}
      <span class="muted small"><kbd>Enter</kbd> submit · <kbd>Shift</kbd>+<kbd>Enter</kbd> new line</span>
    </div>
  {/if}

  {#if attempt}
    {@const g = attempt.grade}
    {@const incomplete = !g.verdict.trim() || !g.best_translation.trim()}
    {@const stale = attempt.line_text !== null && attempt.line_text !== text}
    <div class="grade">
      {#if stale}
        <p class="stale small">
          You fixed this line after this grade. It was graded against “<span lang="es">{attempt.line_text}</span>”.
          Regrade to grade your translation against the corrected line.
        </p>
      {/if}
      <div class="row">
        <span class="score" data-score={g.score}>{g.score}</span>
        <p class="verdict">
          {#if incomplete}
            <span class="muted">The AI's feedback didn't come through for this one. Regrade fetches it again for the same translation.</span>
          {:else}
            {g.verdict}
          {/if}
        </p>
      </div>
      <dl>
        <dt>You wrote</dt>
        <dd>{attempt.translation}</dd>
        <dt>The AI's version</dt>
        <dd class="best">{g.best_translation}</dd>
        {#if g.missed_nuance.length}
          <dt>Missed</dt>
          <dd>
            <ul>
              {#each g.missed_nuance as item}<li>{item}</li>{/each}
            </ul>
          </dd>
        {/if}
      </dl>
      {#if g.skill_tags.length}
        <div class="tags">
          {#each g.skill_tags as tag}<a class="tag" href="/repaso?tag={tag}">{tag}</a>{/each}
        </div>
      {/if}
      {#if !retrying}
        <div class="again">
          <button class="link" onclick={() => { retrying = true; setTimeout(focus); }}>Try again</button>
          <button class="link" onclick={regrade} disabled={regrading} title="Grade this same translation again; doesn't count as a new attempt">
            {regrading ? "Regrading…" : "Regrade"}
          </button>
        </div>
      {/if}
    </div>

    <div class="thread" bind:this={threadEl}>
      {#each thread as m (m.id)}
        <div class="msg {m.role}">{m.content}</div>
      {/each}
      {#if pending}
        <div class="msg user">{pending}</div>
        <div class="msg assistant thinking">Thinking…</div>
      {/if}
      <textarea
        bind:value={question}
        rows="1"
        placeholder="Ask about this line…"
        onkeydown={(e) => onKey(e, ask)}
        disabled={asking}
      ></textarea>
    </div>
  {/if}

  {#if error}<p class="error">{error}</p>{/if}
</section>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .spanish {
    font-size: 1.35rem;
    font-weight: 600;
    margin: 0;
  }
  .spanish-row {
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
  }
  .edit-line {
    flex: none;
    font-size: 1rem;
    text-decoration: none;
  }
  .spanish-edit {
    font-size: 1.2rem;
    font-weight: 600;
  }
  .stale {
    margin: 0;
    padding: 0.5rem 0.65rem;
    border-radius: 8px;
    border: 1px solid var(--accent-2);
  }
  .small {
    font-size: 0.85rem;
    margin: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-wrap: wrap;
  }
  .grade {
    border-top: 1px solid var(--border);
    padding-top: 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .verdict {
    margin: 0;
    flex: 1;
    min-width: 12rem;
  }
  dl {
    margin: 0;
    display: grid;
    gap: 0.15rem 0;
  }
  dt {
    font-size: 0.8rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-top: 0.4rem;
  }
  dd {
    margin: 0;
  }
  dd.best {
    font-weight: 600;
  }
  ul {
    margin: 0;
    padding-left: 1.2rem;
  }
  .tags {
    display: flex;
    gap: 0.35rem;
    flex-wrap: wrap;
  }
  .again {
    display: flex;
    gap: 0.75rem;
  }
  .again .link {
    padding-left: 0;
  }
  .thread {
    border-top: 1px solid var(--border);
    padding-top: 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .msg {
    white-space: pre-wrap;
    padding: 0.5rem 0.75rem;
    border-radius: 10px;
    max-width: 92%;
  }
  .msg.user {
    align-self: flex-end;
    background: var(--accent);
    color: var(--accent-text);
  }
  .msg.assistant {
    align-self: flex-start;
    background: var(--surface-2);
  }
  .msg.thinking {
    color: var(--muted);
    font-style: italic;
  }
</style>
