<script lang="ts">
  export let view: any;

  const laneOrder = ['Main', 'Review', 'Merge'];

  $: laneWorkers = view?.laneWorkers ?? {};
  $: queueIssues = view?.queueIssues ?? [];
  $: issueRows = buildIssueRows(queueIssues, laneWorkers, view);
  $: laneColumns = laneOrder.map((lane) => ({
    lane,
    issues: issueRows.filter((issue) => issue.lane === lane),
    pickedCount: issueRows.filter((issue) => issue.lane === lane && issue.runtimeCategory === 'Active runtime').length,
    completedCount: issueRows.filter((issue) => issue.lane === lane && issue.runtimeCategory === 'Recent completion').length
  }));

  function buildIssueRows(issues: any[] = [], workersByLane: Record<string, any[]> = {}, model: any = {}) {
    const byId = new Map();

    for (const issue of issues ?? []) {
      const id = normalizeIssueRef(issue.id);
      if (!id) continue;
      byId.set(id, baseIssueRow(issue, model));
    }

    for (const [laneKey, workers] of Object.entries(workersByLane ?? {})) {
      for (const worker of workers as any[]) {
        const id = normalizeIssueRef(worker.issue);
        if (!id) continue;
        const existing = byId.get(id) ?? baseIssueRow({ id, title: worker.title, lane: titleCase(laneKey), state: worker.target }, model);
        byId.set(id, {
          ...existing,
          lane: existing.lane || titleCase(laneKey),
          title: worker.title || existing.title,
          worker,
          runtimeCategory: 'Active runtime',
          runtimeTone: 'success',
          runtimeDetail: runtimeDetailForWorker(worker),
          localRuntime: localRuntimeForWorker(worker, model)
        });
      }
    }

    if (!byId.size) return fallbackRows(model);

    return [...byId.values()].map((issue) => ({
      ...issue,
      workpadCategory: workpadCategoryForIssue(issue),
      workpadTone: workpadToneForIssue(issue),
      workpadLink: issue.url
    })).sort(issueSort);
  }

  function baseIssueRow(issue: any, model: any) {
    const state = issue.state ?? 'Unknown';
    const completed = isCompletedIssue(issue);
    return {
      id: normalizeIssueRef(issue.id) ?? issue.id,
      title: issue.title ?? 'Untitled issue',
      lane: issue.lane ?? stateToLane(state),
      state,
      url: issue.url,
      updatedAt: issue.updatedAt,
      evidence: issue.evidence,
      recommended: issue.recommended,
      workerStatus: issue.workerStatus,
      nextSkill: issue.nextSkill,
      runtimeCategory: completed ? 'Recent completion' : runtimeCategoryForIssue(issue),
      runtimeTone: completed ? 'success' : issue.workerStatus === 'Worker read unavailable' ? 'warn' : 'neutral',
      runtimeDetail: completed
        ? 'Local runtime is no longer active; use the issue timeline for closeout evidence.'
        : issue.workerDetail ?? 'No active local runtime is visible for this issue.',
      localRuntime: localRuntimeForIssue(issue, model)
    };
  }

  function fallbackRows(model: any) {
    return (model?.issueIndex ?? []).slice(0, 8).map((issue: any) => ({
      ...baseIssueRow(issue, model),
      workpadCategory: workpadCategoryForIssue(issue),
      workpadTone: workpadToneForIssue(issue),
      workpadLink: issue.url
    }));
  }

  function runtimeCategoryForIssue(issue: any) {
    if (issue.workerStatus === 'Worker matched') return 'Active runtime';
    if (issue.workerStatus === 'Worker read unavailable') return 'Runtime unknown';
    return 'No active runtime';
  }

  function runtimeDetailForWorker(worker: any) {
    return [worker.action, worker.backend, worker.session].filter(Boolean).join(' · ') || 'Worker is visible.';
  }

  function localRuntimeForWorker(worker: any, model: any) {
    return [
      worker.elapsed ? `Elapsed ${worker.elapsed}` : null,
      worker.target ? `Target ${worker.target}` : null,
      model?.raw?.localStatus?.head ? `Head ${model.raw.localStatus.head}` : null
    ].filter(Boolean).join(' · ') || 'Runtime metadata visible through lane worker readback.';
  }

  function localRuntimeForIssue(issue: any, model: any) {
    const local = model?.raw?.localStatus;
    if (isCompletedIssue(issue)) return local?.head ? `Last local head ${local.head}` : 'Completion has no active local runtime.';
    if (issue.workerStatus === 'Worker read unavailable') return 'Session/runtime read unavailable.';
    if (local?.head) return `${local.head} · ${local.dirtyCount ?? 0} dirty · ${local.worktreeCount ?? 0} worktrees`;
    return 'No runtime metadata attached to this issue.';
  }

  function workpadCategoryForIssue(issue: any) {
    const state = normalizeState(issue.state);
    if (state === 'Human Review') return 'Human decision note';
    if (state === 'Agent Review') return 'Review evidence';
    if (state === 'Merging') return 'Merge closeout evidence';
    if (state === 'Need Human Input' || state === 'Need To Clarify') return 'Operator question thread';
    if (state === 'Done') return 'Closeout timeline';
    return 'Main Agent Workpad';
  }

  function workpadToneForIssue(issue: any) {
    if (!issue.url) return 'warn';
    if (['Need Human Input', 'Need To Clarify', 'Human Review'].includes(normalizeState(issue.state))) return 'warn';
    return 'success';
  }

  function isCompletedIssue(issue: any) {
    const state = normalizeState(issue.state);
    return state === 'Done' || /done|merged|completed|closed/i.test(`${issue.evidence ?? ''} ${issue.recommended ?? ''}`);
  }

  function stateToLane(state: string) {
    const normalized = normalizeState(state);
    if (['Agent Review', 'Human Review', 'Need Human Input', 'Need To Clarify'].includes(normalized)) return 'Review';
    if (normalized === 'Merging' || normalized === 'Done') return 'Merge';
    return 'Main';
  }

  function normalizeState(state: unknown) {
    return titleCase(String(state ?? 'Unknown'));
  }

  function normalizeIssueRef(value: unknown) {
    const match = String(value ?? '').match(/#?\d+/);
    return match ? `#${match[0].replace('#', '')}` : null;
  }

  function titleCase(value: unknown) {
    return String(value ?? '')
      .replace(/[-_]/g, ' ')
      .replace(/\b\w/g, (letter) => letter.toUpperCase());
  }

  function issueSort(left: any, right: any) {
    const laneDelta = laneOrder.indexOf(left.lane) - laneOrder.indexOf(right.lane);
    if (laneDelta) return laneDelta;
    const runtimeRank = { 'Active runtime': 0, 'Recent completion': 1, 'Runtime unknown': 2, 'No active runtime': 3 };
    return (runtimeRank[left.runtimeCategory] ?? 9) - (runtimeRank[right.runtimeCategory] ?? 9) ||
      String(left.id).localeCompare(String(right.id), undefined, { numeric: true });
  }
</script>

<section class="route-hero compact">
  <div>
    <p class="eyebrow">Lane Observability</p>
    <h2>Lane Views</h2>
    <p>Issue-first lane posture, local runtime visibility, and workpad routing without duplicating lane pages.</p>
  </div>

  <div class="pagination">
    <span class="section-note">{view?.generatedAtLabel ?? 'not checked'}</span>
  </div>
</section>

<section class="lane-board-overview lane-views-board" aria-label="Lane issue board">
  <div class="lane-board-grid">
    {#each laneColumns as lane}
      <article class="lane-board-column {lane.pickedCount ? 'success' : lane.issues.length ? 'warn' : 'neutral'}">
        <div class="lane-board-column-head">
          <strong>{lane.lane}</strong>
          <small>{lane.pickedCount} active · {lane.completedCount} completed</small>
        </div>

        <div class="lane-board-issue-list">
          {#if lane.issues.length}
            {#each lane.issues as issue}
              <a class="lane-board-item {issue.runtimeTone} {issue.runtimeCategory === 'Active runtime' ? 'picked' : ''}" href={`#${issue.id.replace('#', 'issue-')}`}>
                <strong>{issue.id}</strong>
                <span>{issue.title}</span>
              </a>
            {/each}
          {:else}
            <div class="lane-board-empty">No issue visible.</div>
          {/if}
        </div>
      </article>
    {/each}
  </div>
</section>

<section class="lane-issue-table" aria-label="Issue runtime and workpad status">
  {#if issueRows.length}
    {#each issueRows as issue}
      <article class="lane-issue-row" id={issue.id.replace('#', 'issue-')}>
        <div class="lane-issue-title">
          <span class="issue-tag">{issue.id}</span>
          <div>
            <h3>{issue.title}</h3>
            <p>{issue.state} · {issue.lane}</p>
          </div>
        </div>

        <div class="lane-issue-observability">
          <section>
            <span class="mini-label">Local runtime</span>
            <strong class={issue.runtimeTone}>{issue.runtimeCategory}</strong>
            <p>{issue.runtimeDetail}</p>
            <small>{issue.localRuntime}</small>
          </section>
          <section>
            <span class="mini-label">Workpad status</span>
            <strong class={issue.workpadTone}>{issue.workpadCategory}</strong>
            <p>{issue.recommended ?? issue.nextSkill ?? 'Open the issue timeline before acting.'}</p>
            {#if issue.workpadLink}
              <a class="queue-link" href={issue.workpadLink} target="_blank" rel="noreferrer">Open issue body/comments</a>
            {:else}
              <small>No issue link in current readback.</small>
            {/if}
          </section>
        </div>
      </article>
    {/each}
  {:else}
    <div class="inline-empty">
      <strong>No lane issues visible</strong>
      <p>Refresh live reads or switch to fixture mode to inspect the Lane Views layout.</p>
    </div>
  {/if}
</section>
