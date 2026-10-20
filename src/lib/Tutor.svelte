<script lang="ts">
  import { tick } from "svelte";
  import { api, errorMessage, type TutorMessage } from "$lib/api";
  import { renderMarkdown } from "$lib/markdown";

  const STARTERS = [
    "What's my Spanish level, based on everything so far?",
    "What should I work on next?",
    "Quiz me on my weakest skill.",
    "Explain the mistake I make most often.",
  ];

  let messages = $state<TutorMessage[]>([]);
  let loaded = $state(false);
  let question = $state("");
  let pending = $state<string | null>(null);
  let error = $state("");
  let list = $state<HTMLElement>();
  let input = $state<HTMLTextAreaElement>();

  api.tutorThread().then((m) => {
    messages = m;
    loaded = true;
    reveal("end");
  });

  /** Scrolls the page to the newest message: its start for replies, the very end otherwise. */
  async function reveal(where: "start" | "end") {
    await tick();
    const last = list?.querySelector(".msg:last-of-type");
    last?.scrollIntoView({ block: where === "start" ? "start" : "end", behavior: "smooth" });
  }

  async function ask(text = question) {
    const q = text.trim();
    if (!q || pending) return;
    error = "";
    pending = q;
    question = "";
    reveal("end");
    try {
      messages = [...messages, ...(await api.askTutor(q))];
      pending = null;
      reveal("start");
    } catch (e) {
      question = q;
      pending = null;
      error = errorMessage(e);
    }
  }

  async function startOver() {
    if (!confirm("Start a new conversation? The tutor forgets this chat, but not your scores.")) return;
    await api.clearTutor();
    messages = [];
    input?.focus();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      ask();
    }
  }
</script>

<section class="tutor">
  <p class="muted intro">
    Ask anything about your Spanish. The tutor sees all your graded work (scores, skills, recent translations and what
    you missed) and keeps this conversation going until you start over.
  </p>

  <div class="messages" bind:this={list}>
    {#each messages as m (m.id)}
      {#if m.role === "assistant"}
        <div class="msg assistant">{@html renderMarkdown(m.content)}</div>
      {:else}
        <div class="msg user">{m.content}</div>
      {/if}
    {/each}
    {#if pending}
      <div class="msg user">{pending}</div>
      <div class="msg assistant thinking">Looking over your work…</div>
    {/if}
  </div>

  {#if loaded && messages.length === 0 && !pending}
    <div class="starters">
      {#each STARTERS as s}
        <button onclick={() => ask(s)}>{s}</button>
      {/each}
    </div>
  {/if}

  <div class="compose">
    <textarea
      bind:this={input}
      bind:value={question}
      rows="2"
      placeholder="Ask your tutor… (Enter to send, Shift+Enter for a new line)"
      onkeydown={onKey}
      disabled={pending !== null}
    ></textarea>
    <div class="row">
      <button class="primary" onclick={() => ask()} disabled={pending !== null || !question.trim()}>Send</button>
      {#if messages.length}<button class="link" onclick={startOver}>New conversation</button>{/if}
    </div>
    {#if error}<p class="error">{error}</p>{/if}
  </div>
</section>

<style>
  .tutor {
    max-width: 760px;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .intro {
    margin: 0;
    font-size: 0.9rem;
  }
  .messages {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .msg {
    padding: 0.6rem 0.85rem;
    border-radius: 12px;
    max-width: 92%;
  }
  .msg.user {
    align-self: flex-end;
    background: var(--accent);
    color: var(--accent-text);
    white-space: pre-wrap;
  }
  .msg.assistant {
    align-self: flex-start;
    background: var(--surface);
    border: 1px solid var(--border);
  }
  .msg.thinking {
    color: var(--muted);
    font-style: italic;
  }
  .msg.assistant :global(p) {
    margin: 0 0 0.5rem;
  }
  .msg.assistant :global(p:last-child),
  .msg.assistant :global(ul:last-child),
  .msg.assistant :global(ol:last-child) {
    margin-bottom: 0;
  }
  .msg.assistant :global(ul),
  .msg.assistant :global(ol) {
    margin: 0 0 0.5rem;
    padding-left: 1.3rem;
  }
  .msg.assistant :global(table) {
    border-collapse: collapse;
    margin: 0.25rem 0 0.5rem;
    font-size: 0.92rem;
  }
  .msg.assistant :global(th),
  .msg.assistant :global(td) {
    border: 1px solid var(--border);
    padding: 0.25rem 0.55rem;
    text-align: left;
  }
  .msg.assistant :global(code) {
    background: var(--surface-2);
    padding: 0 0.25rem;
    border-radius: 4px;
  }
  .starters {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  .starters button {
    font-size: 0.9rem;
  }
  .compose {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    position: sticky;
    bottom: 0;
    background: var(--bg);
    padding: 0.5rem 0 0.25rem;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }
  .error {
    margin: 0;
  }
</style>
