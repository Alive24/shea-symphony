<script lang="ts">
  import type { GitHubUserSnapshot } from '../tauriAutoloop.ts';

  type ThemeMode = 'daylight' | 'night';
  type HandoffTarget = 'codex-app' | 'codex-cli' | 'github';
  type RefreshInterval = 'manual' | '10000' | '30000' | '60000';

  export let githubUser: GitHubUserSnapshot;
  export let githubUserLabel = 'gh unavailable';
  export let githubUserDetail = 'GitHub CLI unavailable';
  export let handoffTargets: { id: string; label: string }[] = [];
  export let handoffTarget: HandoffTarget = 'codex-app';
  export let refreshInterval: RefreshInterval = 'manual';
  export let refreshRunning = false;
  export let refreshLabel = 'Refresh';
  export let theme: ThemeMode = 'daylight';
  export let developerToolsOpen = true;
  export let onClose: () => void = () => {};
  export let onHandoffTargetChange: (event: Event) => void = () => {};
  export let onRefresh: () => void = () => {};
  export let onRefreshIntervalChange: (event: Event) => void = () => {};
  export let onToggleTheme: () => void = () => {};
  export let onDeveloperToolsVisibilityChange: (event: Event) => void = () => {};
</script>

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
          <strong>{githubUser.available ? githubUser.name || githubUser.login : 'GitHub CLI unavailable'}</strong>
          <span>{githubUserLabel}</span>
          <small>{githubUserDetail}</small>
        </div>
      </section>

      <section class="settings-section">
        <span class="settings-section-label">Handoff</span>
        <label class="settings-select-row">
          <span>Default destination</span>
          <select value={handoffTarget} onchange={onHandoffTargetChange} aria-label="Default handoff development environment">
            {#each handoffTargets as target}
              <option value={target.id}>{target.label}</option>
            {/each}
          </select>
        </label>
      </section>

      <section class="settings-section">
        <span class="settings-section-label">Refresh</span>
        <div class="settings-refresh-row">
          <button class="refresh-button" type="button" aria-busy={refreshRunning} onclick={onRefresh}>
            {refreshLabel}
          </button>
          <label class="settings-select-row">
            <span>Auto interval</span>
            <select value={refreshInterval} onchange={onRefreshIntervalChange} aria-label="Auto refresh interval">
              <option value="manual">Manual</option>
              <option value="10000">10s</option>
              <option value="30000">30s</option>
              <option value="60000">1m</option>
            </select>
          </label>
        </div>
      </section>

      <section class="settings-section">
        <span class="settings-section-label">Theme</span>
        <button
          class="theme-toggle settings-theme-toggle"
          type="button"
          aria-label="Toggle Day and Night theme"
          aria-pressed={theme === 'night'}
          onclick={onToggleTheme}
        >
          <span>{theme === 'daylight' ? 'Day' : 'Night'}</span>
        </button>
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
