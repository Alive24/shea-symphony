<script lang="ts">
  import type {
    AutoloopControlSnapshot,
    GitHubUserSnapshot,
    RefreshInterval,
    RefreshOption,
    ThemeMode
  } from './NavigatorTypes.ts';

  export let autoloopControl: AutoloopControlSnapshot = {
    tauriAvailable: false,
    busy: false,
    running: false,
    mode: 'dry-run',
    workflowPath: '.shea/workflows/shea-symphony.md',
    latestLine: 'No recent autoloop result',
    laneMaxSummary: ''
  };
  export let refreshRunning = false;
  export let refreshLabel = 'Refresh';
  export let refreshMenuOpen = false;
  export let refreshOptions: RefreshOption[] = [];
  export let refreshInterval: RefreshInterval = 'manual';
  export let selectedRefreshOption: RefreshOption = { value: 'manual', label: 'Manual' };
  export let theme: ThemeMode = 'daylight';
  export let settingsOpen = false;
  export let githubUser: GitHubUserSnapshot = {
    available: false,
    login: '',
    name: '',
    email: '',
    avatarUrl: '',
    error: ''
  };
  export let githubUserLabel = 'gh unavailable';
  export let onStartWrite = () => {};
  export let onStopAutoloop = () => {};
  export let onRequestRefresh = () => {};
  export let onToggleRefreshMenu = () => {};
  export let onSelectRefreshInterval: (value: RefreshInterval) => void = () => {};
  export let onOpenLogs = () => {};
  export let onToggleTheme = () => {};
  export let onOpenSettings = () => {};
</script>

<div class="topbar-cluster nav-actions" aria-label="Runtime state">
  <button
    class="runtime-action-button runtime-action-write"
    type="button"
    disabled={!autoloopControl.tauriAvailable || autoloopControl.busy || autoloopControl.running}
    onclick={onStartWrite}
  >
    Start
  </button>
  <button
    class="runtime-action-button"
    type="button"
    disabled={!autoloopControl.tauriAvailable || autoloopControl.busy || !autoloopControl.running}
    onclick={onStopAutoloop}
  >
    Stop
  </button>
  <div class="runtime-refresh-split">
    <button
      class="runtime-refresh-main"
      type="button"
      aria-busy={refreshRunning}
      disabled={refreshRunning}
      onclick={onRequestRefresh}
    >
      {refreshLabel}
    </button>
    <button
      class="runtime-refresh-menu-button"
      type="button"
      aria-label={`Auto refresh interval: ${selectedRefreshOption.label}`}
      aria-haspopup="listbox"
      aria-expanded={refreshMenuOpen}
      onclick={onToggleRefreshMenu}
    >
      <span class="select-caret" aria-hidden="true"></span>
    </button>
    {#if refreshMenuOpen}
      <div class="refresh-split-menu" role="listbox" aria-label="Auto refresh intervals">
        {#each refreshOptions as option}
          <button
            type="button"
            role="option"
            aria-selected={option.value === refreshInterval}
            onclick={() => onSelectRefreshInterval(option.value)}
          >
            {option.label}
          </button>
        {/each}
      </div>
    {/if}
  </div>
  <button class="runtime-action-button" type="button" onclick={onOpenLogs}>Logs</button>
  <button
    class="theme-icon-button"
    type="button"
    aria-label="Toggle Day and Night theme"
    aria-pressed={theme === 'night'}
    onclick={onToggleTheme}
  >
    {#if theme === 'daylight'}
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="12" r="4"></circle>
        <path d="M12 2v3M12 19v3M4.93 4.93l2.12 2.12M16.95 16.95l2.12 2.12M2 12h3M19 12h3M4.93 19.07l2.12-2.12M16.95 7.05l2.12-2.12"></path>
      </svg>
    {:else}
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M20.2 14.4A7.6 7.6 0 0 1 9.6 3.8A8.7 8.7 0 1 0 20.2 14.4Z"></path>
      </svg>
    {/if}
  </button>
  <button
    class="menu-button"
    type="button"
    aria-label={`Settings ${githubUserLabel}`}
    aria-haspopup="dialog"
    aria-expanded={settingsOpen}
    onclick={onOpenSettings}
  >
    {#if githubUser.avatarUrl}
      <img src={githubUser.avatarUrl} alt="" />
    {:else}
      <span class="menu-avatar" aria-hidden="true">gh</span>
    {/if}
    <span class="menu-user-id">{githubUserLabel}</span>
  </button>
</div>
