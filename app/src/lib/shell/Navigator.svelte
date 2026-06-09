<script lang="ts">
  import NavigatorActions from './NavigatorActions.svelte';
  import NavigatorBrand from './NavigatorBrand.svelte';
  import NavigatorLinks from './NavigatorLinks.svelte';
  import type {
    AutoloopControlSnapshot,
    GitHubUserSnapshot,
    NavigatorItem,
    NavigatorNavigateHandler,
    RefreshInterval,
    RefreshOption,
    ThemeMode
  } from './NavigatorTypes.ts';

  export let currentPath = '/';
  export let navItems: NavigatorItem[] = [];
  export let refreshRunning = false;
  export let refreshRemaining = 0;
  export let refreshFinishedAt: string | null = null;
  export let refreshLabel = 'Refresh';
  export let refreshMenuOpen = false;
  export let refreshOptions: RefreshOption[] = [];
  export let refreshInterval: RefreshInterval = 'manual';
  export let selectedRefreshOption: RefreshOption = { value: 'manual', label: 'Manual' };
  export let autoloopControl: AutoloopControlSnapshot;
  export let theme: ThemeMode = 'daylight';
  export let settingsOpen = false;
  export let githubUser: GitHubUserSnapshot;
  export let githubUserLabel = 'gh unavailable';
  export let onNavigate: NavigatorNavigateHandler = () => {};
  export let onStartWrite = () => {};
  export let onStopAutoloop = () => {};
  export let onRequestRefresh = () => {};
  export let onToggleRefreshMenu = () => {};
  export let onSelectRefreshInterval: (value: RefreshInterval) => void = () => {};
  export let onOpenLogs = () => {};
  export let onToggleTheme = () => {};
  export let onOpenSettings = () => {};
</script>

<header class="rail" aria-label="Primary navigation">
  <NavigatorBrand
    {refreshRunning}
    {refreshRemaining}
    {refreshFinishedAt}
    {onNavigate}
  />

  <NavigatorLinks items={navItems} {currentPath} {onNavigate} />

  <NavigatorActions
    {autoloopControl}
    {refreshRunning}
    {refreshLabel}
    {refreshMenuOpen}
    {refreshOptions}
    {refreshInterval}
    {selectedRefreshOption}
    {theme}
    {settingsOpen}
    {githubUser}
    {githubUserLabel}
    {onStartWrite}
    {onStopAutoloop}
    {onRequestRefresh}
    {onToggleRefreshMenu}
    {onSelectRefreshInterval}
    {onOpenLogs}
    {onToggleTheme}
    {onOpenSettings}
  />
</header>
