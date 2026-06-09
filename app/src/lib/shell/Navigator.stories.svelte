<script module>
  import { defineMeta } from '@storybook/addon-svelte-csf';
  import { fn } from 'storybook/test';
  import Navigator from './Navigator.svelte';
  import NavigatorActions from './NavigatorActions.svelte';
  import NavigatorBrand from './NavigatorBrand.svelte';
  import NavigatorLinks from './NavigatorLinks.svelte';

  const navItems = [
    { href: '/', label: 'Operator Desk' },
    { href: '/lanes', label: 'Lanes' },
    { href: '/doctor', label: 'Doctor' },
    { href: '/intelligence', label: 'Intelligence' }
  ];

  const refreshOptions = [
    { value: 'manual', label: 'Manual' },
    { value: '10000', label: '10s' },
    { value: '30000', label: '30s' },
    { value: '60000', label: '1m' }
  ];

  const autoloopControl = {
    tauriAvailable: true,
    busy: false,
    running: false,
    mode: 'dry-run',
    workflowPath: 'workflows/shea-symphony.md',
    latestLine: 'No recent autoloop result',
    laneMaxSummary: ''
  };

  const runningAutoloopControl = {
    ...autoloopControl,
    running: true,
    latestLine: 'manual loop running issue #421'
  };

  const githubUser = {
    available: true,
    login: 'operator',
    name: 'Operator',
    email: 'operator@example.com',
    avatarUrl: '',
    error: ''
  };

  const navigateSpy = fn();
  const startWriteSpy = fn();
  const stopAutoloopSpy = fn();
  const requestRefreshSpy = fn();
  const toggleRefreshMenuSpy = fn();
  const selectRefreshIntervalSpy = fn();
  const openLogsSpy = fn();
  const toggleThemeSpy = fn();
  const openSettingsSpy = fn();

  const storyNavigate = (event, href) => {
    event.preventDefault();
    navigateSpy(href);
  };

  const { Story } = defineMeta({
    title: 'Components/Navigator',
    component: Navigator,
    tags: ['autodocs'],
    parameters: {
      layout: 'fullscreen'
    },
    args: {
      currentPath: '/',
      navItems,
      refreshRunning: false,
      refreshRemaining: 0,
      refreshFinishedAt: new Date('2026-06-09T09:30:00Z').toISOString(),
      refreshLabel: 'Refresh',
      refreshMenuOpen: false,
      refreshOptions,
      refreshInterval: 'manual',
      selectedRefreshOption: refreshOptions[0],
      autoloopControl,
      theme: 'daylight',
      settingsOpen: false,
      githubUser,
      githubUserLabel: '@operator',
      onNavigate: storyNavigate,
      onStartWrite: startWriteSpy,
      onStopAutoloop: stopAutoloopSpy,
      onRequestRefresh: requestRefreshSpy,
      onToggleRefreshMenu: toggleRefreshMenuSpy,
      onSelectRefreshInterval: selectRefreshIntervalSpy,
      onOpenLogs: openLogsSpy,
      onToggleTheme: toggleThemeSpy,
      onOpenSettings: openSettingsSpy
    }
  });
</script>

{#snippet fullTemplate(args)}
  <div class="navigator-story-shell">
    <main class="navigator-story-app-frame" aria-label="Storybook 1920 by 1080 app shell preview">
      <Navigator {...args} />
    </main>
  </div>
{/snippet}

{#snippet brandTemplate(args)}
  <div class="navigator-story-shell">
    <main class="navigator-story-part-frame navigator-story-brand-frame" aria-label="Navigator refresh status preview">
      <header class="rail navigator-story-part-rail" aria-label="Navigator left section">
        <NavigatorBrand
          refreshRunning={args.refreshRunning}
          refreshRemaining={args.refreshRemaining}
          refreshFinishedAt={args.refreshFinishedAt}
          onNavigate={args.onNavigate}
        />
      </header>
    </main>
  </div>
{/snippet}

{#snippet linksTemplate(args)}
  <div class="navigator-story-shell">
    <main class="navigator-story-part-frame navigator-story-links-frame" aria-label="Navigator links preview">
      <header class="rail navigator-story-part-rail" aria-label="Navigator middle section">
        <NavigatorLinks
          items={args.navItems}
          currentPath={args.currentPath}
          onNavigate={args.onNavigate}
        />
      </header>
    </main>
  </div>
{/snippet}

{#snippet actionsTemplate(args)}
  <div class="navigator-story-shell">
    <main class="navigator-story-part-frame navigator-story-actions-frame" aria-label="Navigator operation area preview">
      <header class="rail navigator-story-part-rail" aria-label="Navigator right section">
        <NavigatorActions
          autoloopControl={args.autoloopControl}
          refreshRunning={args.refreshRunning}
          refreshLabel={args.refreshLabel}
          refreshMenuOpen={args.refreshMenuOpen}
          refreshOptions={args.refreshOptions}
          refreshInterval={args.refreshInterval}
          selectedRefreshOption={args.selectedRefreshOption}
          theme={args.theme}
          settingsOpen={args.settingsOpen}
          githubUser={args.githubUser}
          githubUserLabel={args.githubUserLabel}
          onStartWrite={args.onStartWrite}
          onStopAutoloop={args.onStopAutoloop}
          onRequestRefresh={args.onRequestRefresh}
          onToggleRefreshMenu={args.onToggleRefreshMenu}
          onSelectRefreshInterval={args.onSelectRefreshInterval}
          onOpenLogs={args.onOpenLogs}
          onToggleTheme={args.onToggleTheme}
          onOpenSettings={args.onOpenSettings}
        />
      </header>
    </main>
  </div>
{/snippet}

<Story name="Full navigator" template={fullTemplate} />
<Story
  name="Left refresh status"
  args={{ refreshRunning: true, refreshRemaining: 3, refreshFinishedAt: new Date('2026-06-09T09:31:00Z').toISOString() }}
  template={brandTemplate}
/>
<Story name="Center navigation links" args={{ currentPath: '/doctor' }} template={linksTemplate} />
<Story
  name="Right operation area"
  args={{
    autoloopControl: runningAutoloopControl,
    refreshMenuOpen: true,
    refreshInterval: '30000',
    selectedRefreshOption: refreshOptions[2],
    theme: 'night'
  }}
  template={actionsTemplate}
/>

<style>
  .navigator-story-shell {
    min-height: 100vh;
    overflow: auto;
    color: var(--fg);
    background: var(--bg);
  }

  .navigator-story-app-frame {
    width: 1920px;
    height: 1080px;
    background: var(--bg);
  }

  .navigator-story-part-frame {
    padding: var(--space-8);
  }

  .navigator-story-part-rail {
    position: static;
    width: fit-content;
  }

  .navigator-story-brand-frame .navigator-story-part-rail {
    min-width: 360px;
  }

  .navigator-story-links-frame .navigator-story-part-rail {
    width: 720px;
  }

  .navigator-story-actions-frame .navigator-story-part-rail {
    min-width: 760px;
    justify-content: flex-end;
  }

  @media (max-width: 720px) {
    .navigator-story-part-frame {
      padding: var(--space-4);
    }
  }
</style>
