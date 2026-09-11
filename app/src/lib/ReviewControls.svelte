<script lang="ts">
  import { onMount } from 'svelte';
  import { isTauriRuntime } from './tauriAutoloop.ts';
  import { getIssueReviewStatus, getReviewActionState, prepareReviewAction, executeReviewAction, reviewLifecycle,
    type ReviewAction, type ReviewActionState, type ReviewPreview, type ReviewStatus } from './reviewActions.ts';

  export let issue: string;
  export let workspace: string;
  let status: ReviewStatus | null = null;
  let operation: ReviewActionState = { running: false };
  let preview: ReviewPreview | null = null;
  let busy = false;
  let error = '';
  let mounted = false;
  let polling = false;
  const native = isTauriRuntime();
  $: lifecycle = reviewLifecycle(status, issue);
  $: activeHere = operation.running && operation.issue === issue && operation.workspace === workspace;

  function message(value: unknown): string { return value instanceof Error ? value.message : String(value); }
  async function refresh() {
    if (!native || busy) return;
    busy = true;
    preview = null;
    error = '';
    try { status = await getIssueReviewStatus(issue); }
    catch (value) { status = null; error = message(value); }
    finally { busy = false; }
  }
  async function prepare(action: ReviewAction) {
    busy = true;
    preview = null;
    error = '';
    try { preview = await prepareReviewAction(issue, action); }
    catch (value) { error = message(value); }
    finally { busy = false; }
  }
  async function execute() {
    if (!preview) return;
    const token = preview.token;
    preview = null;
    busy = true;
    error = '';
    try { operation = await executeReviewAction(token); }
    catch (value) { error = message(value); }
    finally { busy = false; }
  }
  onMount(() => {
    mounted = true;
    void refresh();
    const timer = window.setInterval(async () => {
      if (!native || polling) return;
      polling = true;
      try {
        const next = await getReviewActionState();
        if (!mounted) return;
        const completed = operation.running && !next.running;
        operation = next;
        if (completed) await refresh();
      } catch (value) { if (mounted) error = message(value); }
      finally { polling = false; }
    }, 2000);
    return () => { mounted = false; window.clearInterval(timer); };
  });
</script>

<section class="review-controls" aria-label="Independent Review">
  <div class="review-heading">
    <div><strong>Independent Review</strong><p>{activeHere ? 'Review command in progress' : lifecycle.label}</p></div>
    <button class="btn btn-ghost" type="button" disabled={!native || busy} on:click={refresh}>Refresh Review</button>
  </div>
  <p>{activeHere ? 'The CLI is preparing, reviewing or publishing this Issue. Read the run receipt to verify backend progress.' : lifecycle.detail}</p>
  {#if lifecycle.runId}<small>Run: {lifecycle.runId}</small>{/if}
  {#if !native}<p>Open the desktop App to check or run Review.</p>{/if}
  {#if operation.issue === issue && operation.workspace === workspace && operation.result}
    <p class:error={!operation.result.ok}>{operation.result.ok ? 'Command finished. Review the refreshed publication status.' : operation.result.stderr || 'The Review command failed. Inspect the retained run evidence.'}</p>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <div class="review-buttons">
    <button class="btn btn-primary" type="button" disabled={!native || busy || operation.running || !lifecycle.canStart} on:click={() => prepare('once')}>Prepare Review</button>
    <button class="btn btn-ghost" type="button" disabled={!native || busy || operation.running || !lifecycle.canRecover} on:click={() => prepare('recover')}>Prepare recovery</button>
  </div>
  {#if preview}
    <div class="review-preview" role="region" aria-label="Review operation preview">
      <strong>{preview.action === 'once' ? `Start one independent Review for ${issue}` : `Recover evidence publication for ${issue}`}</strong>
      <p>{preview.action === 'once' ? 'This claims Review, runs the configured independent backend, publishes its evidence and routes the Issue.' : 'This publishes the captured result and completes routing without launching another reviewer.'}</p>
      <small>{preview.workspace}</small>
      <pre>{preview.summary.stdoutPreview}</pre>
      <button class="btn btn-primary" type="button" disabled={busy} on:click={execute}>{preview.action === 'once' ? 'Confirm and start Review' : 'Confirm recovery'}</button>
      <button class="btn btn-ghost" type="button" on:click={() => preview = null}>Cancel</button>
    </div>
  {/if}
</section>

<style>
  .review-controls { padding: 1.25rem; margin: 0 0 1.25rem; border: 1px solid var(--border-color, #d9d9e2); border-radius: 12px; }
  .review-heading, .review-buttons { display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; }
  .review-buttons { justify-content: flex-start; margin-top: 1rem; }
  p { margin: 0.5rem 0; }
  small { display: block; overflow-wrap: anywhere; opacity: 0.8; }
  .error { color: #c63a43; white-space: pre-wrap; overflow-wrap: anywhere; }
  .review-preview { margin-top: 1rem; padding-top: 1rem; border-top: 1px solid var(--border-color, #d9d9e2); }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 12rem; overflow: auto; font-size: 0.8rem; }
</style>
