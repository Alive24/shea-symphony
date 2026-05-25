<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import {
    HANDOFF_TARGETS,
    HANDOFF_TARGET_CHANGE_EVENT,
    defaultHandoffTargetStore,
    getDataMode,
    getDefaultHandoffTarget,
    resetFixtureOverview,
    setDataMode,
    setDefaultHandoffTarget
  } from '$lib/api';

  type ThemeMode = 'daylight' | 'night';
  type DataMode = 'live' | 'fixture';
  type HandoffTarget = 'codex-app' | 'codex-cli' | 'github';

  const navItems = [
    { label: 'Operator Desk', href: '/' },
    { label: 'Lanes', href: '/lanes' },
    { label: 'Events', href: '/events' },
    { label: 'Runbook', href: '/runbook' },
    { label: 'Doctor', href: '/doctor' },
    { label: 'Settings', href: '/settings' }
  ];

  let theme: ThemeMode = 'daylight';
  let dataMode: DataMode = 'live';
  let handoffTarget: HandoffTarget = 'codex-app';
  $: path = $page.url.pathname;

  function applyTheme(nextTheme: ThemeMode) {
    theme = nextTheme;
    document.documentElement.dataset.theme = nextTheme;
    localStorage.setItem('shea-theme', nextTheme);
  }

  function toggleTheme() {
    applyTheme(theme === 'daylight' ? 'night' : 'daylight');
  }

  function toggleDataMode() {
    dataMode = dataMode === 'fixture' ? 'live' : 'fixture';
    setDataMode(dataMode);
  }

  function resetFixture() {
    dataMode = 'fixture';
    setDataMode('fixture');
    resetFixtureOverview();
  }

  function updateHandoffTarget(event: Event) {
    handoffTarget = (event.currentTarget as HTMLSelectElement).value as HandoffTarget;
    setDefaultHandoffTarget(handoffTarget);
    window.dispatchEvent(new CustomEvent(HANDOFF_TARGET_CHANGE_EVENT, { detail: { target: handoffTarget } }));
  }

  function isActive(href: string) {
    if (href === '/') return path === '/';
    return path.startsWith(href);
  }

  onMount(() => {
    const savedTheme = localStorage.getItem('shea-theme');
    applyTheme(savedTheme === 'night' ? 'night' : 'daylight');
    dataMode = getDataMode();
    handoffTarget = getDefaultHandoffTarget() as HandoffTarget;
    defaultHandoffTargetStore.set(handoffTarget);
  });
</script>

<svelte:head>
  <title>Shea Symphony Operator Desk</title>
  <meta
    name="description"
    content="A high-fidelity local foreground workflow cockpit for Shea Symphony."
  />
</svelte:head>

<div class="app-chrome">
  <header class="rail" aria-label="Primary navigation">
    <a class="brand-lockup" href="/">
      <span class="brand-mark" aria-hidden="true">SS</span>
      <span>
        <strong>Shea Symphony</strong>
        <small>Operator Desk</small>
      </span>
    </a>

    <nav class="nav-list">
      {#each navItems as item}
        <a class:active={isActive(item.href)} href={item.href}>{item.label}</a>
      {/each}
    </nav>

    <div class="rail-foot">
      <span class="mini-label">Foreground</span>
      <span class="rail-state">Local loop armed</span>
    </div>

    <div class="topbar-cluster nav-actions" aria-label="Runtime state">
      <label class="handoff-default">
        <span>Handoff</span>
        <select value={handoffTarget} onchange={updateHandoffTarget} aria-label="Default handoff development environment">
          {#each HANDOFF_TARGETS as target}
            <option value={target.id}>{target.label}</option>
          {/each}
        </select>
      </label>
      <button
        class="mode-switch"
        type="button"
        aria-label="Toggle Live and Fixture data"
        aria-pressed={dataMode === 'fixture'}
        onclick={toggleDataMode}
      >
        <span>{dataMode === 'fixture' ? 'Fixture' : 'Live'}</span>
      </button>
      {#if dataMode === 'fixture'}
        <button class="mode-reset" type="button" onclick={resetFixture}>Reset</button>
      {/if}
      <span class="countdown">Manual refresh</span>
      <span class="health-pill">Writes gated</span>
      <button
        class="theme-toggle"
        type="button"
        aria-label="Toggle Day and Night theme"
        aria-pressed={theme === 'night'}
        onclick={toggleTheme}
      >
        <span>{theme === 'daylight' ? 'Day' : 'Night'}</span>
      </button>
    </div>
  </header>

  <section class="workspace">
    <main class="screen-shell">
      <slot />
    </main>
  </section>
</div>
