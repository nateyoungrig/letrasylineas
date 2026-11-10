<script lang="ts">
  import "../app.css";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { api, errorMessage, type UserList } from "$lib/api";
  import { savedPalette } from "$lib/palette";

  document.documentElement.dataset.palette = savedPalette();

  let { children } = $props();
  let hasKey = $state(true);

  $effect(() => {
    page.url.pathname; // re-check after visiting Settings
    api.aiStatus().then((s) => { hasKey = s.connections.some((c) => c.provider === s.settings.provider && c.connected); }).catch(() => { hasKey = false; });
  });

  let users = $state<UserList | null>(null);
  api.listUsers().then((u) => (users = u));

  async function switchUser(id: string) {
    try {
      // Song ids belong to one user's library, so leave a song before switching.
      if (page.url.pathname.startsWith("/song")) await goto("/");
      users = await api.switchUser(id);
    } catch (e) {
      alert(errorMessage(e));
    }
  }

  // Pages load their data when they mount, so a new user remounts the page. The
  // transcriber is left alone: a draft in progress is saved to whoever is current.
  const viewKey = $derived(page.url.pathname.startsWith("/transcribe") ? "transcribe" : users?.current);

  const nav = [
    { href: "/", label: "Library" },
    { href: "/repaso", label: "Repaso" },
    { href: "/settings", label: "Settings" },
  ];

  // The lyric board fits the window and scrolls inside its own panes.
  const fit = $derived(page.url.pathname.startsWith("/song"));

  function active(href: string) {
    const path = page.url.pathname;
    return href === "/" ? path === "/" || path.startsWith("/song") : path.startsWith(href);
  }
</script>

<div class="shell">
<header>
  <a class="brand" href="/">Letras <span>y</span> Líneas</a>
  <nav>
    {#each nav as item}
      <a href={item.href} aria-current={active(item.href) ? "page" : undefined}>{item.label}</a>
    {/each}
    {#if users}
      <select
        class="user"
        aria-label="User"
        value={users.current}
        onchange={(e) => switchUser(e.currentTarget.value)}
      >
        {#each users.users as user (user.id)}
          <option value={user.id}>{user.name}</option>
        {/each}
      </select>
    {/if}
  </nav>
</header>

{#if !hasKey && !page.url.pathname.startsWith("/settings")}
  <p class="banner">Connect an AI provider to start grading. <a href="/settings">Choose one in Settings</a>.</p>
{/if}

<main class:fit>
  <div class="content">
    {#key viewKey}
      {@render children()}
    {/key}
  </div>
</main>
</div>

<style>
  .shell {
    height: 100vh;
    height: 100dvh;
    display: flex;
    flex-direction: column;
  }
  header {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  .brand {
    font-weight: 800;
    font-size: 1.15rem;
    text-decoration: none;
    letter-spacing: -0.01em;
    white-space: nowrap;
  }
  .brand span {
    color: var(--accent-2);
    font-style: italic;
  }
  nav {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }
  .user {
    margin-left: 0.5rem;
    font: inherit;
    color: inherit;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 0.35rem 0.5rem;
  }
  nav a {
    text-decoration: none;
    padding: 0.4rem 0.7rem;
    border-radius: 8px;
    color: var(--muted);
  }
  nav a[aria-current="page"] {
    color: var(--text);
    background: var(--surface-2);
    font-weight: 600;
  }
  .banner {
    flex: none;
    margin: 0;
    padding: 0.6rem 1rem;
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
    font-size: 0.95rem;
  }
  @media (max-width: 420px) {
    nav a {
      padding: 0.4rem 0.5rem;
    }
  }
  main {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  main.fit {
    overflow: hidden;
  }
  .content {
    max-width: 1200px;
    margin: 0 auto;
    padding: 1.25rem 1rem 4rem;
  }
  main.fit .content {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding-bottom: 1rem;
  }
</style>
