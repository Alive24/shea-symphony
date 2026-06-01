<script lang="ts">
  import type { GitHubUserSnapshot } from '../tauriAutoloop.ts';

  type HandoffTarget = 'codex-app' | 'claude-code' | 'gemini-cli';
  type RefreshInterval = 'manual' | '10000' | '30000' | '60000';
  type HandoffIcon = 'codex' | 'claude' | 'gemini';

  export let githubUser: GitHubUserSnapshot;
  export let githubUserLabel = 'gh unavailable';
  export let githubUserDetail = 'GitHub CLI unavailable';
  export let handoffTargets: { id: string; label: string; icon?: string }[] = [];
  export let handoffTarget: HandoffTarget = 'codex-app';
  export let refreshInterval: RefreshInterval = 'manual';
  export let refreshRunning = false;
  export let refreshLabel = 'Refresh';
  export let developerToolsOpen = true;
  export let onClose: () => void = () => {};
  export let onHandoffTargetChange: (event: Event) => void = () => {};
  export let onHandoffTargetSelect: (target: HandoffTarget) => void = () => {};
  export let onRefresh: () => void = () => {};
  export let onRefreshIntervalChange: (event: Event) => void = () => {};
  export let onDeveloperToolsVisibilityChange: (event: Event) => void = () => {};

  let handoffMenuOpen = false;
  let refreshMenuOpen = false;
  const refreshOptions = [
    { value: 'manual', label: 'Manual' },
    { value: '10000', label: '10s' },
    { value: '30000', label: '30s' },
    { value: '60000', label: '1m' }
  ];

  $: selectedHandoffTarget =
    handoffTargets.find((target) => target.id === handoffTarget) ?? handoffTargets[0];
  $: selectedRefreshOption =
    refreshOptions.find((option) => option.value === refreshInterval) ?? refreshOptions[0];
  $: accountName = githubUser.name?.trim() ?? '';
  $: accountLogin = githubUser.login?.trim() ?? '';
  $: showAccountName = Boolean(
    githubUser.available &&
      accountName &&
      accountLogin &&
      accountName.toLowerCase() !== accountLogin.toLowerCase()
  );
  $: accountPrimary = githubUser.available
    ? showAccountName
      ? accountName
      : githubUserLabel
    : 'GitHub CLI unavailable';

  function selectHandoffTarget(target: string) {
    handoffMenuOpen = false;
    onHandoffTargetSelect(target as HandoffTarget);
  }

  function selectRefreshInterval(value: RefreshInterval) {
    refreshMenuOpen = false;
    onRefreshIntervalChange({ currentTarget: { value } } as unknown as Event);
  }

  function iconName(icon: string | undefined): HandoffIcon {
    if (icon === 'claude' || icon === 'gemini') return icon;
    return 'codex';
  }
</script>

{#snippet handoffIcon(icon: string | undefined)}
  {#if iconName(icon) === 'claude'}
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M12 3 3.7 21h3.7l1.7-3.9h5.8l1.7 3.9h3.7L12 3Z"></path>
      <path d="m10.2 14.1 1.8-4.3 1.8 4.3h-3.6Z"></path>
    </svg>
  {:else if iconName(icon) === 'gemini'}
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M12 2.8c1.1 5 4.2 8.1 9.2 9.2-5 1.1-8.1 4.2-9.2 9.2-1.1-5-4.2-8.1-9.2-9.2 5-1.1 8.1-4.2 9.2-9.2Z"></path>
    </svg>
  {:else}
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M11.7 3.2a4.2 4.2 0 0 1 4.1 2.6 4.2 4.2 0 0 1 4.9 5.9 4.2 4.2 0 0 1-2.6 4.1 4.2 4.2 0 0 1-5.9 4.9 4.2 4.2 0 0 1-4.1-2.6 4.2 4.2 0 0 1-4.9-5.9 4.2 4.2 0 0 1 2.6-4.1 4.2 4.2 0 0 1 5.9-4.9Z"></path>
      <path d="M8.4 8.4h7.2v7.2H8.4z"></path>
    </svg>
  {/if}
{/snippet}

<div class="modal-backdrop">
  <button class="modal-scrim" type="button" aria-label="Close settings" onclick={onClose}></button>
  <div class="cli-log-modal settings-modal" role="dialog" aria-modal="true" aria-label="Settings">
    <div class="settings-modal-body">
      <section class="settings-account">
        {#if githubUser.avatarUrl}
          <img src={githubUser.avatarUrl} alt="" />
        {:else}
          <span class="settings-account-avatar" aria-hidden="true">gh</span>
        {/if}
        <div>
          <strong>{accountPrimary}</strong>
          {#if showAccountName}
            <span>{githubUserLabel}</span>
          {/if}
          <small>{githubUserDetail}</small>
        </div>
      </section>

      <section class="settings-section settings-section-inline">
        <span class="settings-section-label">Handoff</span>
        <div class="handoff-picker">
          <select class="native-handoff-fallback" value={handoffTarget} onchange={onHandoffTargetChange} aria-label="Default handoff development environment" tabindex="-1">
            {#each handoffTargets as target}
              <option value={target.id}>{target.label}</option>
            {/each}
          </select>
          <button
            class="handoff-picker-button"
            type="button"
            aria-label="Default handoff development environment"
            aria-haspopup="listbox"
            aria-expanded={handoffMenuOpen}
            onclick={() => (handoffMenuOpen = !handoffMenuOpen)}
          >
            <span class={`handoff-icon handoff-icon-${iconName(selectedHandoffTarget?.icon)}`}>{@render handoffIcon(selectedHandoffTarget?.icon)}</span>
            <strong>{selectedHandoffTarget?.label ?? 'App'}</strong>
            <span class="select-caret" aria-hidden="true"></span>
          </button>
          {#if handoffMenuOpen}
            <div class="handoff-picker-menu" role="listbox" aria-label="Handoff destinations">
              {#each handoffTargets as target}
                <button
                  type="button"
                  role="option"
                  aria-selected={target.id === handoffTarget}
                  onclick={() => selectHandoffTarget(target.id)}
                >
                  <span class={`handoff-icon handoff-icon-${iconName(target.icon)}`}>{@render handoffIcon(target.icon)}</span>
                  <span>{target.label}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </section>

      <section class="settings-section settings-section-inline settings-refresh-section">
        <span class="settings-section-label">Refresh</span>
        <div class="settings-refresh-row">
          <button class="refresh-button" type="button" aria-busy={refreshRunning} onclick={onRefresh}>
            {refreshLabel}
          </button>
          <div class="settings-picker refresh-picker">
            <select class="native-picker-fallback" value={refreshInterval} onchange={onRefreshIntervalChange} aria-label="Auto refresh interval" tabindex="-1">
              {#each refreshOptions as option}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
            <button
              class="settings-picker-button"
              type="button"
              aria-label="Auto refresh interval"
              aria-haspopup="listbox"
              aria-expanded={refreshMenuOpen}
              onclick={() => (refreshMenuOpen = !refreshMenuOpen)}
            >
              <strong>{selectedRefreshOption.label}</strong>
              <span class="select-caret" aria-hidden="true"></span>
            </button>
            {#if refreshMenuOpen}
              <div class="settings-picker-menu" role="listbox" aria-label="Auto refresh intervals">
                {#each refreshOptions as option}
                  <button
                    type="button"
                    role="option"
                    aria-selected={option.value === refreshInterval}
                    onclick={() => selectRefreshInterval(option.value as RefreshInterval)}
                  >
                    {option.label}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        </div>
      </section>

      <label class="settings-checkbox">
        <input type="checkbox" checked={developerToolsOpen} onchange={onDeveloperToolsVisibilityChange} />
        <span>
          <strong>Developer Tools</strong>
          <small>Show the debug sidebar on the right side of the app.</small>
        </span>
      </label>
    </div>
  </div>
</div>
