<script lang="ts">
  import { tick } from "svelte";
  import { api, errorMessage, type ChatTurn, type DraftLine, type Suggestion } from "$lib/api";
  import { renderMarkdown } from "$lib/markdown";

  let {
    title,
    artist,
    lines,
    line,
    suggestions,
    suspicious,
    messages = $bindable(),
  }: {
    title: string;
    artist: string;
    /** The draft as it stands when a question is sent. */
    lines: () => DraftLine[];
    /** Index of the line under discussion. */
    line: number;
    /** Claude's pending changes to this line, if any. */
    suggestions: Suggestion[];
    suspicious: string | null;
    messages: ChatTurn[];
  } = $props();

  let question = $state("");
  let pending = $state<string | null>(null);
  let error = $state("");
  let input = $state<HTMLTextAreaElement>();

  tick().then(() => input?.focus());

  async function ask() {
    const q = question.trim();
    if (!q || pending) return;
    error = "";
    pending = q;
    question = "";
    try {
      const plain = suggestions.map(({ line, original, replacement, reason }) => ({ line, original, replacement, reason }));
      const reply = await api.discussLine(title, artist, lines(), line, plain, suspicious, messages, q);
      messages = [...messages, { role: "user", content: q }, { role: "assistant", content: reply }];
    } catch (e) {
      question = q;
      error = errorMessage(e);
    } finally {
      pending = null;
      await tick();
      input?.focus();
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      ask();
    }
  }
</script>

<div class="discussion">
  {#each messages as m}
    {#if m.role === "assistant"}
      <div class="msg assistant">{@html renderMarkdown(m.content)}</div>
    {:else}
      <div class="msg user">{m.content}</div>
    {/if}
  {/each}
  {#if pending}
    <div class="msg user">{pending}</div>
    <div class="msg assistant thinking">Thinking it over…</div>
  {/if}
  <textarea
    bind:this={input}
    bind:value={question}
    rows="2"
    placeholder={messages.length ? "Reply… (Enter to send)" : "What do you hear, or what's unclear? (Enter to send)"}
    onkeydown={onKey}
    disabled={pending !== null}
  ></textarea>
  {#if error}<p class="error small">{error}</p>{/if}
</div>

<style>
  .discussion {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    margin: 0 0 0.6rem 2.9rem;
    padding: 0.6rem;
    border: 1px solid var(--border);
    border-radius: 10px;
    max-width: 640px;
  }
  .msg {
    padding: 0.45rem 0.7rem;
    border-radius: 10px;
    max-width: 92%;
    font-size: 0.92rem;
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
    margin: 0 0 0.4rem;
  }
  .msg.assistant :global(p:last-child),
  .msg.assistant :global(ul:last-child) {
    margin-bottom: 0;
  }
  .msg.assistant :global(ul),
  .msg.assistant :global(ol) {
    margin: 0 0 0.4rem;
    padding-left: 1.2rem;
  }
  .error {
    margin: 0;
  }
  .small {
    font-size: 0.85rem;
  }
  @media (max-width: 800px) {
    .discussion {
      margin-left: 0.5rem;
    }
  }
</style>
