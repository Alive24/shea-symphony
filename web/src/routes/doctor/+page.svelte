<script lang="ts">
  import { onMount } from 'svelte';
  import CommandHealthPanel from '$lib/CommandHealthPanel.svelte';
  import ReadPathMap from '$lib/ReadPathMap.svelte';
  import ReadSurfaceObservatory from '$lib/ReadSurfaceObservatory.svelte';
  import { DATA_MODE_CHANGE_EVENT, buildViewModel, loadOverview, loadReadSurface, mergeReadSurface, runCommand } from '$lib/api';

  let loading = true;
  let liveError = '';
  let commandIssue = '';
  let commandAction = 'project-issue';
  let commandMarkdown = '';
  let commandTitle = '';
  let commandAssignees = '';
  let forgeStatus = 'Todo';
  let doctorRepairAction = 'inspect';
  let reviewRejectTarget = 'agent_review';
  let commandBusy = false;
  let commandResult = null;
  let fullLoading = false;
  let slowReadsRemaining = 0;
  let view = buildViewModel(null);

  const commandOptions = [
    ['project-issue', 'Project issue readback'],
    ['project-inspect', 'Project inspect'],
    ['quality-gate', 'Quality gate dry-run'],
    ['autopilot-plan', 'Autopilot plan'],
    ['doctor', 'Doctor'],
    ['review-status', 'Review status'],
    ['skills-status', 'Skills status'],
    ['session-list', 'Session list'],
    ['workspace-list', 'Workspace list'],
    ['clean-audit', 'Clean audit'],
    ['forge-validate', 'Forge validate']
  ];

  $: commandHealth = view.commandHealth ?? [];
  $: readPathMap = view.readPathMap ?? [];
  $: commandNeedsIssue = [
    'project-issue',
    'project-inspect',
    'quality-gate',
    'forge-validate'
  ].includes(commandAction);
  $: commandNeedsMarkdown = false;
  $: commandNeedsTitle = commandAction === 'forge-validate' && !commandIssue.trim();
  $: commandReady =
    (!commandNeedsIssue || /^#?\d+$/.test(commandIssue.trim())) &&
    (!commandNeedsMarkdown || commandMarkdown.trim().length > 0) &&
    (!commandNeedsTitle || commandTitle.trim().length > 0);
  $: commandPreview = previewCommand(commandAction, commandIssue);

  async function refresh(force = false) {
    loading = true;
    liveError = '';
    try {
      view = buildViewModel(await loadOverview(force, 'fast'));
      const slowSurfaces = ['autopilot', 'doctor', 'review', 'local'];
      fullLoading = true;
      slowReadsRemaining = slowSurfaces.length;
      await Promise.allSettled(
        slowSurfaces.map(async (name) => {
          try {
            const surface = await loadReadSurface(name, force);
            view = buildViewModel(mergeReadSurface(view.raw, surface));
          } finally {
            slowReadsRemaining -= 1;
          }
        })
      );
    } catch (error) {
      liveError = error.message;
      view = buildViewModel(null);
    } finally {
      loading = false;
      fullLoading = false;
      slowReadsRemaining = 0;
    }
  }

  async function runSelectedCommand() {
    commandBusy = true;
    commandResult = null;
    try {
      commandResult = await runCommand({
        action: commandAction,
        issue: commandIssue,
        state: 'Need Human Input',
        targetState: reviewRejectTarget,
        markdown: commandMarkdown,
        title: commandTitle,
        assignees: commandAssignees,
        forgeStatus,
        repairAction: doctorRepairAction,
        write: false
      });
      if (commandResult.ok) await refresh(true);
    } catch (error) {
      commandResult = { ok: false, stderr: error.message, stdout: '' };
    } finally {
      commandBusy = false;
    }
  }

  function previewCommand(action, issue) {
    const workflow = view.workflowPath ?? 'workflows/shea-symphony.md';
    const normalizedIssue = issue.trim() || '#<issue>';
    const commands = {
      'project-issue': ['shea-symphony', 'project', 'issue', workflow, normalizedIssue, '--json'],
      'project-inspect': ['shea-symphony', 'project', 'inspect', workflow, normalizedIssue],
      'quality-gate': ['shea-symphony', 'gate', workflow, normalizedIssue, '--dry-run'],
      'autopilot-plan': ['shea-symphony', 'autopilot', 'plan', workflow, '--json'],
      doctor: ['shea-symphony', 'doctor', workflow, '--json'],
      'review-status': ['shea-symphony', 'review', 'status', workflow, '--json'],
      'skills-status': ['shea-symphony', 'skills', 'status', workflow, '--json'],
      'session-list': ['shea-symphony', 'session', 'list', workflow],
      'workspace-list': ['shea-symphony', 'workspace', 'list', workflow],
      'clean-audit': ['shea-symphony', 'clean', 'audit', workflow],
      'forge-validate': [
        'shea-symphony',
        'forge',
        'validate',
        '--workflow',
        workflow,
        '--status',
        forgeStatus,
        ...(issue.trim() ? ['--issue', normalizedIssue] : []),
        ...(commandTitle.trim() ? ['--title', commandTitle.trim()] : []),
        ...(commandMarkdown.trim() ? ['--body-file', '<markdown>'] : [])
      ]
    };
    return (commands[action] ?? ['shea-symphony']).join(' ');
  }

  onMount(() => {
    refresh();
    const dataModeListener = () => refresh(true);
    window.addEventListener(DATA_MODE_CHANGE_EVENT, dataModeListener);
    return () => window.removeEventListener(DATA_MODE_CHANGE_EVENT, dataModeListener);
  });
</script>

<section class="diagnostics-intro">
  <div>
    <p class="eyebrow">Doctor</p>
    <h2>Diagnostic Readback</h2>
    <p>Use this for exact read commands and dry-run previews. Project mutations and routing decisions stay in chat Skills.</p>
  </div>
  <button class="btn btn-ghost" type="button" on:click={() => refresh(true)} disabled={loading}>
    {loading ? 'Refreshing' : fullLoading ? `${slowReadsRemaining} reads left` : 'Refresh'}
  </button>
</section>

{#if liveError}
  <div class="inline-alert">
    <strong>Live API unavailable</strong>
    <span>{liveError}</span>
  </div>
{/if}

<ReadSurfaceObservatory commands={commandHealth} />
<ReadPathMap paths={readPathMap} />
<CommandHealthPanel commands={commandHealth} />

<section class="command-console" aria-labelledby="command-console-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">Read-only CLI</p>
      <h2 id="command-console-title">Readback Console</h2>
    </div>
    <span class="status-pill success">No writes</span>
  </div>

  <form class="command-form" on:submit|preventDefault={runSelectedCommand}>
    <label class="field">
      <span>Action</span>
      <select id="command-action" name="command-action" bind:value={commandAction}>
        {#each commandOptions as [value, label]}
          <option {value}>{label}</option>
        {/each}
      </select>
    </label>

    <label class="field">
      <span>Issue</span>
      <input id="command-issue" name="command-issue" bind:value={commandIssue} placeholder="#123" disabled={!commandNeedsIssue} />
    </label>

    {#if commandAction === 'forge-validate'}
      <label class="field">
        <span>Forge status</span>
        <select id="forge-status" name="forge-status" bind:value={forgeStatus}>
          <option>Todo</option>
          <option>Backlog</option>
        </select>
      </label>
    {/if}

    <button class="btn btn-primary" type="submit" disabled={commandBusy || !commandReady}>
      {commandBusy ? 'Reading' : 'Read'}
    </button>
  </form>

  {#if !commandReady}
    <p class="form-hint">
      {commandNeedsIssue && !/^#?\d+$/.test(commandIssue.trim())
        ? 'Enter an issue number like #123 before running this command.'
        : commandNeedsTitle && !commandTitle.trim()
          ? 'Enter a title before running this command.'
          : 'Enter evidence markdown before running this command.'}
    </p>
  {/if}

  {#if commandAction === 'forge-validate'}
    <div class="command-extra-grid">
      <label class="field">
        <span>Title</span>
        <input id="command-title" name="command-title" bind:value={commandTitle} placeholder="Executable issue title" />
      </label>
    </div>
  {/if}

  <div class="command-preview">
    <span>Read/dry-run command</span>
    <code>{commandPreview}</code>
  </div>

  {#if commandResult}
    <article class:danger={!commandResult.ok} class="command-output">
      <div>
        <strong>{commandResult.ok ? 'Command succeeded' : 'Command failed'}</strong>
        {#if commandResult.durationMs}
          <span>{Math.round(commandResult.durationMs / 1000)}s</span>
        {/if}
      </div>
      <pre>{commandResult.stdout || commandResult.stderr || commandResult.error}</pre>
    </article>
  {/if}
</section>
