<script lang="ts">
  import { autoloopStateStore, REFRESH_REQUEST_EVENT } from './uiState.ts';
  import { operatorOverviewStore } from './operatorOverviewStore.ts';
  import {
    localArtifactRefreshEventDetail
  } from './localArtifactRefresh.ts';
  import { getCodexTranscript, getIssueTimeline, openCodexThread, openGitHubSource } from './tauriAutoloop.ts';
  import {
    classifyHeartbeat,
    transcriptUnavailable
  } from './viewModel/codexTranscript.ts';
  import {
    completedProgressDisplay
  } from './viewModel/completedWorktrees.ts';
  import {
    buildIssueCommentLifecycleEvents
  } from './viewModel/githubIssueTimeline.ts';
  import {
    issueIdentityTitle,
    nonTransientIssueTitle
  } from './viewModel/issueTitles.ts';

  export let view: any;
  export let route = '/lanes';

  const laneOrder = ['Human Todo', 'Main', 'Review', 'Merge'];
  const humanStates = new Set(['Need To Clarify', 'Need Human Input', 'Human Review']);
  const completedWindows = [
    { label: 'All', hours: null },
    { label: '1h', hours: 1 },
    { label: '3h', hours: 3 },
    { label: '6h', hours: 6 },
    { label: '12h', hours: 12 },
    { label: '24h', hours: 24 },
    { label: '72h', hours: 72 }
  ];
  const completedPageSize = 10;

  let completedWindowHours = null;
  let completedPage = 1;
  let transcriptLoading = false;
  let transcriptError = '';
  let transcriptResponse: any = transcriptUnavailable('Transcript has not been loaded yet.');
  let transcriptLoadKey = '';
  let issueTimelineLoading = false;
  let issueTimelineError = '';
  let issueTimelineResponse: any = null;
  let issueTimelineLoadKey = '';
  let sourceLinkError = '';

  $: laneWorkers = view?.laneWorkers ?? {};
  $: queueIssues = view?.queueIssues ?? [];
  $: issueRows = buildIssueRows(queueIssues, laneWorkers, view);
  $: selectedIssueRef = routeIssueRef(route);
  $: completedLocalIssues = buildCompletedLocalIssues(view, issueRows);
  $: titledCompletedLocalIssues = completedLocalIssues.filter(hasCompleteIssueTitle);
  $: filteredCompletedLocalIssues = filterCompletedByWindow(titledCompletedLocalIssues, completedWindowHours);
  $: completedPageCount = Math.max(1, Math.ceil(filteredCompletedLocalIssues.length / completedPageSize));
  $: completedPage = Math.min(completedPage, completedPageCount);
  $: pagedCompletedLocalIssues = filteredCompletedLocalIssues.slice((completedPage - 1) * completedPageSize, completedPage * completedPageSize);
  $: completedPageStart = filteredCompletedLocalIssues.length ? (completedPage - 1) * completedPageSize + 1 : 0;
  $: completedPageEnd = Math.min(completedPage * completedPageSize, filteredCompletedLocalIssues.length);
  $: selectedIssue = selectedIssueRef ? findIssueForDetail(selectedIssueRef, issueRows, completedLocalIssues, view) : null;
  $: localArtifactsRefresh = $operatorOverviewStore.localArtifactsRefresh;
  $: remoteLifecycleEvents = selectedIssue ? buildIssueCommentLifecycleEvents(issueTimelineResponse, selectedIssue) : [];
  $: lifecycleEvents = selectedIssue ? buildLifecycleEvents(selectedIssue, view, remoteLifecycleEvents) : [];
  $: selectedLaneKey = laneKeyForIssue(selectedIssue);
  $: heartbeatSummary = selectedIssue ? classifyHeartbeat($autoloopStateStore, selectedLaneKey, selectedIssue.id, Date.now(), laneEventEvidence(selectedIssue, view, remoteLifecycleEvents)) : null;
  $: eventLogPath = localEventLogPath(view);
  $: transcriptStatusLabel = transcriptResponse?.status === 'available'
    ? transcriptResponse?.deepLink ? 'Codex App link ready' : 'Conversation found locally'
    : transcriptResponse?.reason ?? 'Transcript unavailable';
  $: transcriptDeepLink = transcriptResponse?.deepLink ?? null;
  $: maybeLoadIssueTimeline(selectedIssue);
  $: maybeLoadTranscript(selectedIssue);
  $: laneColumns = laneOrder.map((lane) => {
    const issues = lane === 'Human Todo'
      ? issueRows.filter((issue) => humanStates.has(normalizeState(issue.state)))
      : issueRows.filter((issue) => issue.lane === lane && !humanStates.has(normalizeState(issue.state)));
    return {
      lane,
      issues,
      pickedCount: issues.filter((issue) => issue.runtimeCategory === 'Active runtime').length,
      completedCount: issues.filter((issue) => issue.runtimeCategory === 'Recent completion').length
    };
  });

  function buildIssueRows(issues: any[] = [], workersByLane: Record<string, any[]> = {}, model: any = {}) {
    const byId = new Map();

    for (const issue of issues ?? []) {
      const id = normalizeIssueRef(issue.id ?? issue.identifier ?? issue.number);
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
          lane: humanStates.has(normalizeState(existing.state)) ? 'Human Todo' : existing.lane || titleCase(laneKey),
          title: issueIdentityTitle(id, [existing.title, worker.title], 'Issue'),
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
    const state = issue.state ?? issue.status ?? 'Unknown';
    const completed = isCompletedIssue(issue);
    const id = normalizeIssueRef(issue.id ?? issue.identifier ?? issue.number) ?? issue.id;
    return {
      id,
      number: issue.number ?? issueNumber(id),
      title: issueIdentityTitle(id, [issue.title], 'Issue'),
      lane: humanStates.has(normalizeState(state)) ? 'Human Todo' : issue.lane ?? stateToLane(state),
      state,
      createdAt: issue.createdAt ?? issue.created_at,
      url: issue.url ?? issue.htmlUrl,
      updatedAt: issue.updatedAt ?? issue.updated_at,
      evidence: issue.evidence,
      recommended: issue.recommended,
      workerStatus: issue.workerStatus,
      runtimeCategory: completed ? 'Recent completion' : runtimeCategoryForIssue(issue),
      runtimeTone: completed ? 'success' : issue.workerStatus === 'Worker read unavailable' ? 'warn' : humanStates.has(normalizeState(state)) ? 'warn' : 'neutral',
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

  function buildCompletedLocalIssues(model: any, rows: any[]) {
    const byId = new Map();
    for (const entry of model?.raw?.localStatus?.completedIssueWorktrees ?? []) {
      const id = normalizeIssueRef(entry.issue ?? entry.issueRef ?? entry.id);
      if (!id) continue;
      byId.set(id, normalizeCompletedEntry(entry, rows, model));
    }
    for (const entry of model?.raw?.localStatus?.issueWorktrees ?? []) {
      const id = normalizeIssueRef(entry.issue ?? entry.issueRef ?? entry.id);
      if (!id) continue;
      const row = rows.find((issue) => issue.id === id);
      const existing = byId.get(id);
      if (existing) {
        byId.set(id, { ...existing, worktree: { ...existing.worktree, ...entry } });
      } else if (row && isCompletedIssue(row)) {
        byId.set(id, normalizeCompletedEntry(entry, rows, model));
      } else if (!row || row.runtimeCategory !== 'Active runtime') {
        byId.set(id, normalizeCompletedEntry({ ...entry, state: row?.state, completedAt: entry.lastModified }, rows, model));
      }
    }
    return [...byId.values()].sort((left, right) => completedLocalSortMs(right) - completedLocalSortMs(left));
  }

  function normalizeCompletedEntry(entry: any, rows: any[], model: any) {
    const id = normalizeIssueRef(entry.issue ?? entry.issueRef ?? entry.id);
    const row = rows.find((issue) => issue.id === id) ?? (model?.issueIndex ?? []).find((issue: any) => normalizeIssueRef(issue.id ?? issue.identifier) === id);
    const issueUrl = entry.url ?? row?.url ?? githubIssueUrl(id);
    const lastProgressAt = entry.lastProgressAt ?? null;
    const completedAt = entry.completedAt ?? lastProgressAt ?? null;
    const lastModified = entry.lastModified ?? null;
    const timestampSources = entry.timestampSources ?? {};
    const lastProgressSource = entry.lastProgressSource ??
      timestampSources?.lastProgress?.source ??
      (lastProgressAt ? 'read_surface.lastProgressAt' : 'unavailable');
    return {
      id,
      title: issueIdentityTitle(id, [entry.title, row?.title], 'Issue'),
      state: entry.state ?? row?.state ?? 'Done',
      lane: entry.lane ?? row?.lane ?? 'Merge',
      createdAt: entry.createdAt ?? row?.createdAt,
      url: issueUrl,
      completedAt,
      updatedAt: entry.updatedAt ?? entry.projectUpdatedAt ?? row?.updatedAt,
      projectUpdatedAt: entry.projectUpdatedAt ?? entry.updatedAt,
      worktree: {
        path: entry.path ?? entry.worktreePath,
        branch: entry.branch,
        head: entry.head,
        createdAt: entry.createdAt,
        lastProgressAt,
        lastProgressSource,
        lastModified,
        lastModifiedSource: entry.lastModifiedSource ?? timestampSources?.lastModified?.source,
        timestampSources,
        treeState: entry.treeState ?? 'unknown',
        diskBytes: entry.diskBytes,
        evidence: entry.evidence
      }
    };
  }

  function hasCompleteIssueTitle(issue: any) {
    const title = nonTransientIssueTitle(issue?.title, issue?.id);
    if (!title) return false;
    const normalized = title.trim().toLowerCase();
    const issueRef = normalizeIssueRef(issue?.id);
    if (normalized === 'issue') return false;
    if (/^issue\s+#?\d+$/.test(normalized)) return false;
    return !issueRef || normalized !== `issue ${issueRef.toLowerCase()}`;
  }

  function setCompletedWindow(hours: number | null) {
    completedWindowHours = hours;
    completedPage = 1;
  }

  function previousCompletedPage() {
    completedPage = Math.max(1, completedPage - 1);
  }

  function nextCompletedPage() {
    completedPage = Math.min(completedPageCount, completedPage + 1);
  }

  function filterCompletedByWindow(issues: any[], hours: number | null) {
    if (hours == null) return issues;
    const cutoff = Date.now() - hours * 60 * 60 * 1000;
    return issues.filter((issue) => {
      const timestamp = completedLocalSortMs(issue);
      return timestamp && timestamp >= cutoff;
    });
  }

  function completedLocalSortMs(issue: any) {
    return dateMs(issue.completedAt) || dateMs(issue.worktree?.lastModified);
  }

  function findIssueForDetail(issueRef: string, rows: any[], completedIssues: any[], model: any) {
    const normalized = normalizeIssueRef(issueRef);
    return completedIssues.find((issue) => issue.id === normalized) ??
      rows.find((issue) => issue.id === normalized) ??
      (model?.issueIndex ?? []).find((issue: any) => normalizeIssueRef(issue.id ?? issue.identifier) === normalized) ??
      { id: normalized, title: 'Issue', state: 'Unknown', url: githubIssueUrl(normalized) };
  }

  function buildLifecycleEvents(issue: any, model: any, remoteEvents: any[] = []) {
    const fixtureEvents = (model?.raw?.localStatus?.issueLifecycle?.[issue.id] ?? model?.raw?.localStatus?.issueLifecycle?.[issue.id?.replace('#', '')] ?? []).map((event: any) => ({
      label: event.label ?? event.phase ?? 'Lifecycle event',
      phase: event.phase ?? event.label ?? 'Timeline',
      time: event.time ?? event.at,
      detail: event.detail ?? '',
      url: event.url ?? issue.url,
      source: event.source ?? 'localStatus.issueLifecycle'
    }));
    const baseEvents = fixtureEvents.length ? fixtureEvents : inferLifecycleEvents(issue, model, remoteEvents);
    const events = [...baseEvents, ...(remoteEvents ?? [])];
    return dedupeEvents(events).sort((left, right) => eventSortMs(left) - eventSortMs(right));
  }

  function laneEventEvidence(issue: any, model: any, remoteEvents: any[] = []) {
    const localLifecycle = model?.raw?.localStatus?.issueLifecycle ?? {};
    const persistedEvents = localLifecycle?.[issue?.id] ?? localLifecycle?.[issue?.id?.replace('#', '')] ?? [];
    const issueEvents = (persistedEvents ?? []).map((event: any) => ({
      ...event,
      issue: issue?.id,
      lane: laneKeyForIssue(issue),
      source: event.source ?? 'localStatus.issueLifecycle'
    }));
    const fullEvents = (model?.fullEvents ?? [])
      .filter((event: any) => eventMentionsIssue(event, issue?.id))
      .map((event: any) => ({
        ...event,
        issue: event.issue ?? event.issueRef ?? issue?.id,
        lane: event.lane ?? laneKeyForIssue(issue),
        time: event.timestamp ?? event.time ?? event.at,
        label: event.title ?? event.label,
        source: event.source ?? 'operator.fullEvents'
      }));
    return [...issueEvents, ...fullEvents, ...(remoteEvents ?? [])];
  }

  function eventMentionsIssue(event: any, issueRef: string) {
    if (!issueRef) return false;
    const normalized = normalizeIssueRef(event?.issue ?? event?.issueRef ?? event?.identifier);
    if (normalized === issueRef) return true;
    const text = `${event?.title ?? ''} ${event?.label ?? ''} ${event?.detail ?? ''}`;
    return text.includes(issueRef);
  }

  function inferLifecycleEvents(issue: any, model: any, remoteEvents: any[] = []) {
    const events: any[] = [];
    const issueUrl = issue.url ?? githubIssueUrl(issue.id);
    if (!hasRemotePhase(remoteEvents, 'Backlog')) {
      events.push({ phase: 'Backlog', label: 'Issue visible in tracker', time: issue.createdAt ?? null, detail: 'Tracker issue readback is available.', url: issueUrl });
    }
    if (issue.lane && issue.lane !== 'Unknown') {
      events.push({ phase: 'Promoted', label: `Promoted into ${issue.lane}`, time: issue.promotedAt ?? promotionTimeFromRemote(remoteEvents) ?? null, detail: issue.state ?? 'Lane state visible.', url: issueUrl });
    }
    for (const event of model?.fullEvents ?? []) {
      const text = `${event.title ?? ''} ${event.detail ?? ''}`;
      if (!text.includes(issue.id)) continue;
      events.push({
        phase: phaseFromText(text, event.lane),
        label: event.title ?? 'Timeline event',
        time: event.timestamp ?? event.time ?? event.at ?? null,
        detail: event.detail ?? '',
        url: event.url ?? issueUrl
      });
    }
    if (isCompletedIssue(issue) && !(remoteEvents ?? []).some((event) => event.phase === 'Done')) {
      events.push({ phase: 'Done', label: 'Completed locally', time: issue.completedAt ?? null, detail: issue.worktree?.path ? 'Local worktree is still present.' : 'Completion is visible in tracker readback.', url: issueUrl });
    }
    return events;
  }

  function promotionTimeFromRemote(events: any[] = []) {
    return firstEventTime(events, ['Promoted', 'Main', 'Handoff']);
  }

  function hasRemotePhase(events: any[] = [], phase: string) {
    return events.some((event) => event.phase === phase && String(event.source ?? '').startsWith('github.'));
  }

  function firstEventTime(events: any[] = [], phases: string[]) {
    let winner: any = null;
    let winnerMs = Number.POSITIVE_INFINITY;
    for (const event of events) {
      if (!phases.includes(event.phase)) continue;
      const ms = eventSortMs(event);
      if (ms && ms < winnerMs) {
        winner = event.time;
        winnerMs = ms;
      }
    }
    return winner;
  }

  function phaseFromText(text: string, lane: string) {
    if (/rework/i.test(text)) return 'Rework';
    if (/human review|human input|clarify/i.test(text)) return 'Human Review';
    if (/agent review|review/i.test(text)) return 'Agent Review';
    if (/merge|done|land/i.test(text)) return 'Merge';
    if (/promote|todo|main|workpad/i.test(text)) return 'Main';
    return lane ?? 'Timeline';
  }

  function dedupeEvents(events: any[]) {
    const seen = new Set();
    return events.filter((event) => {
      const key = `${event.phase}|${event.label}|${event.time}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  }

  function runtimeCategoryForIssue(issue: any) {
    if (issue.workerStatus === 'Worker matched') return 'Active runtime';
    if (issue.workerStatus === 'Worker read unavailable') return 'Runtime unknown';
    return 'No active runtime';
  }

  function runtimeDetailForWorker(worker: any) {
    if (worker.backend === 'Codex app-server') {
      const session = worker.sessionId ?? (worker.session === 'session pending' ? null : worker.session);
      return [
        worker.backend,
        worker.pid ? `PID ${worker.pid}` : null,
        session ? `session ${session}` : 'session pending'
      ].filter(Boolean).join(' · ');
    }
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
    if (local?.issueWorktrees?.length) return `${local.issueWorktrees.length} local issue worktrees`;
    return 'No runtime metadata attached to this issue.';
  }

  function localEventLogPath(model: any) {
    return model?.raw?.localStatus?.eventLogPath ??
      model?.raw?.localStatus?.snapshot?.event_log_path ??
      model?.raw?.localStatus?.snapshot?.eventLogPath ??
      null;
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
    if (humanStates.has(normalizeState(issue.state))) return 'warn';
    return 'success';
  }

  function isCompletedIssue(issue: any) {
    const state = normalizeState(issue.state);
    return state === 'Done' || state === 'Merged' || /done|merged|completed|closed/i.test(`${issue.evidence ?? ''} ${issue.recommended ?? ''} ${issue.runtimeCategory ?? ''}`);
  }

  function stateToLane(state: string) {
    const normalized = normalizeState(state);
    if (humanStates.has(normalized) || normalized === 'Agent Review') return 'Review';
    if (normalized === 'Merging' || normalized === 'Done' || normalized === 'Merged') return 'Merge';
    return 'Main';
  }

  function navigate(event: MouseEvent, href: string) {
    event.preventDefault();
    window.dispatchEvent(new CustomEvent('shea-navigate', { detail: { href } }));
  }

  function refreshLocalArtifacts() {
    window.dispatchEvent(new CustomEvent(REFRESH_REQUEST_EVENT, {
      detail: localArtifactRefreshEventDetail('lane-overview-local')
    }));
    reloadTranscript();
  }

  function maybeLoadTranscript(issue: any) {
    const key = issue ? `${issue.id}|${sessionIdForIssue(issue) ?? ''}` : '';
    if (!key || key === transcriptLoadKey) return;
    transcriptLoadKey = key;
    loadTranscript(issue);
  }

  function maybeLoadIssueTimeline(issue: any) {
    const key = issue?.id ?? '';
    if (!key || key === issueTimelineLoadKey) return;
    issueTimelineLoadKey = key;
    loadIssueTimeline(issue);
  }

  async function loadIssueTimeline(issue: any) {
    issueTimelineLoading = true;
    issueTimelineError = '';
    issueTimelineResponse = null;
    try {
      issueTimelineResponse = await getIssueTimeline(issue.id);
      if (issueTimelineResponse?.available === false) {
        issueTimelineError = issueTimelineResponse?.error ?? 'GitHub issue comments are unavailable.';
      }
    } catch (error) {
      issueTimelineError = error instanceof Error ? error.message : String(error);
      issueTimelineResponse = null;
    } finally {
      issueTimelineLoading = false;
    }
  }

  async function loadTranscript(issue: any) {
    transcriptLoading = true;
    transcriptError = '';
    transcriptResponse = transcriptUnavailable('Loading local Codex transcript.');
    try {
      transcriptResponse = await getCodexTranscript(issue.id, sessionIdForIssue(issue)) ??
        transcriptUnavailable('Codex transcript reads are only available in the desktop shell.');
    } catch (error) {
      transcriptError = error instanceof Error ? error.message : String(error);
      transcriptResponse = transcriptUnavailable(transcriptError);
    } finally {
      transcriptLoading = false;
    }
  }

  function reloadTranscript() {
    if (!selectedIssue) return;
    transcriptLoadKey = '';
    maybeLoadTranscript(selectedIssue);
  }

  async function openCodexTranscript() {
    if (!transcriptDeepLink) return;
    transcriptError = '';
    try {
      await openCodexThread(transcriptDeepLink);
    } catch (error) {
      transcriptError = error instanceof Error ? error.message : String(error);
    }
  }

  async function openSourceLink(url: string) {
    sourceLinkError = '';
    try {
      await openGitHubSource(url);
    } catch (error) {
      sourceLinkError = error instanceof Error ? error.message : String(error);
    }
  }

  function sessionIdForIssue(issue: any) {
    const session = issue?.worker?.sessionId ?? issue?.worker?.session ?? issue?.worktree?.session ?? issue?.session ?? null;
    return session === 'session pending' ? null : session;
  }

  function laneKeyForIssue(issue: any) {
    const lane = String(issue?.lane ?? '').toLowerCase();
    if (lane.includes('review')) return 'review';
    if (lane.includes('merge')) return 'merge';
    return 'main';
  }

  function routeIssueRef(path: string) {
    const match = path.match(/^\/lanes\/(\d+)/);
    return match ? `#${match[1]}` : null;
  }

  function issuePath(issue: any) {
    return `/lanes/${String(issue.id ?? '').replace('#', '')}`;
  }

  function issueNumber(value: unknown) {
    const match = String(value ?? '').match(/\d+/);
    return match ? Number(match[0]) : null;
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

  function dateMs(value: unknown) {
    if (!value) return 0;
    if (typeof value === 'number') return value;
    const parsed = Date.parse(String(value));
    return Number.isNaN(parsed) ? 0 : parsed;
  }

  function eventSortMs(event: any) {
    return dateMs(event?.sortTime ?? event?.time);
  }

  function formatTime(value: unknown) {
    const ms = dateMs(value);
    if (!ms) return 'unknown';
    return new Date(ms).toLocaleString([], { month: 'short', day: '2-digit', hour: '2-digit', minute: '2-digit' });
  }

  function formatTimeWithRelative(value: unknown) {
    const label = formatTime(value);
    if (label === 'unknown') return label;
    return `${label} (${relativeAge(value)})`;
  }

  function relativeAge(value: unknown) {
    const ms = dateMs(value);
    if (!ms) return 'unknown';
    const minutes = Math.max(0, Math.round((Date.now() - ms) / 60000));
    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.round(minutes / 60);
    if (hours < 48) return `${hours}h ago`;
    return `${Math.round(hours / 24)}d ago`;
  }

  function progressAge(issue: any) {
    return completedProgressDisplay(issue, relativeAge);
  }

  function formatMsTime(value: unknown) {
    const ms = typeof value === 'number' ? value : Number(value);
    if (!Number.isFinite(ms) || ms < Date.UTC(2020, 0, 1)) return 'unknown';
    return new Date(ms).toLocaleString([], { month: 'short', day: '2-digit', hour: '2-digit', minute: '2-digit' });
  }

  function shortHead(value: unknown) {
    const text = String(value ?? '').trim();
    return text ? text.slice(0, 7) : 'unknown';
  }

  function formatBytes(value: unknown) {
    const bytes = typeof value === 'number' ? value : Number(value);
    if (!Number.isFinite(bytes) || bytes < 0) return 'unknown';
    if (bytes < 1024) return `${bytes} B`;
    const units = ['KB', 'MB', 'GB', 'TB'];
    let amount = bytes / 1024;
    let unitIndex = 0;
    while (amount >= 1024 && unitIndex < units.length - 1) {
      amount /= 1024;
      unitIndex += 1;
    }
    return `${amount >= 10 ? amount.toFixed(1) : amount.toFixed(2)} ${units[unitIndex]}`;
  }

  function treeStateLabel(value: unknown) {
    const text = String(value ?? 'unknown').trim().toLowerCase();
    if (text === 'clean') return 'Clean';
    if (text === 'dirty') return 'Dirty';
    return 'Unknown';
  }

  function githubIssueUrl(issueRef: unknown) {
    const number = issueNumber(issueRef);
    return number ? `https://github.com/Alive24/shea-symphony/issues/${number}` : undefined;
  }
</script>

{#if selectedIssue}
  <section class="route-hero compact">
    <div>
      <p class="eyebrow">Lane Views</p>
      <h2>{selectedIssue.id} Lifecycle</h2>
      <p>{selectedIssue.title}</p>
    </div>

    <div class="pagination">
      <span class="section-note">{view?.generatedAtLabel ?? 'not checked'}</span>
      <button class="btn btn-ghost" type="button" on:click={refreshLocalArtifacts} disabled={localArtifactsRefresh?.running}>
        {localArtifactsRefresh?.running ? 'Refreshing local artifacts' : 'Refresh local artifacts'}
      </button>
      <a class="btn btn-ghost" href="/lanes" on:click={(event) => navigate(event, '/lanes')}>Back</a>
    </div>
  </section>

  <section class="lane-detail-shell" aria-label={`${selectedIssue.id} lifecycle`}>
    <div class="lane-detail-summary">
      <div>
        <span class="mini-label">State</span>
        <strong>{selectedIssue.state ?? 'Unknown'}</strong>
      </div>
      <div>
        <span class="mini-label">Local worktree</span>
        <strong>{selectedIssue.worktree?.path ? 'Preserved' : 'Not visible'}</strong>
      </div>
      <div>
        <span class="mini-label">Last event</span>
        <strong>{formatTime(selectedIssue.worktree?.lastProgressAt ?? selectedIssue.worktree?.lastModified ?? selectedIssue.completedAt)}</strong>
      </div>
      <div>
        <span class="mini-label">Disk size</span>
        <strong>{formatBytes(selectedIssue.worktree?.diskBytes)}</strong>
      </div>
    </div>

    {#if selectedIssue.worktree?.path}
      <div class="lane-worktree-strip">
        <span>{selectedIssue.worktree.branch ?? 'branch unknown'}</span>
        <strong>{selectedIssue.worktree.head ?? 'head unknown'}</strong>
        <code>{selectedIssue.worktree.path}</code>
      </div>
    {/if}

    <section class="lane-session-panel" aria-label={`${selectedIssue.id} session observability`}>
      <div class="lane-session-head">
        <div>
          <span class="mini-label">Heartbeat / session</span>
          <h3>{heartbeatSummary?.label ?? 'Heartbeat unavailable'}</h3>
        </div>
        <span class="status-pill {heartbeatSummary?.tone ?? 'warn'}">{heartbeatSummary?.state ?? 'unavailable'}</span>
      </div>

      <div class="lane-session-grid">
        <div>
          <span>Last heartbeat</span>
          <strong>{formatMsTime(heartbeatSummary?.lastHeartbeatMs)}</strong>
          <small>{heartbeatSummary?.lastHeartbeatAge ?? 'unknown'}</small>
        </div>
        <div>
          <span>Latest lane event</span>
          <strong>{heartbeatSummary?.latestLaneEvent ?? 'No lane event visible.'}</strong>
          <small>
            {heartbeatSummary?.latestLaneEventAtMs
              ? `${formatMsTime(heartbeatSummary.latestLaneEventAtMs)} · ${heartbeatSummary.latestLaneEventAge}`
              : heartbeatSummary?.latestLaneEventAge ?? 'unknown'}
            {' · '}
            {heartbeatSummary?.issue ? `${heartbeatSummary.lane} · ${heartbeatSummary.issue}` : `${heartbeatSummary?.lane ?? selectedLaneKey} lane`}
          </small>
        </div>
        <div>
          <span>Codex conversation</span>
          <strong>{transcriptLoading ? 'Loading local file' : transcriptStatusLabel}</strong>
          <small>{transcriptResponse?.path ?? 'Local-only diagnostic surface'}</small>
        </div>
        <div>
          <span>Event log</span>
          <strong>{eventLogPath ? 'Local JSONL available' : 'Unavailable locally'}</strong>
          <small>{eventLogPath ?? 'Refresh local artifacts to read runtime log path.'}</small>
        </div>
      </div>
    </section>

    <section class="transcript-panel" aria-label={`${selectedIssue.id} Codex conversation`}>
      <div class="transcript-panel-head">
        <div>
          <span class="mini-label">Codex conversation</span>
          <h3>{transcriptResponse?.status === 'available' ? 'Open in Codex App' : 'Unavailable locally'}</h3>
        </div>
        <div class="transcript-actions">
          <button class="btn btn-ghost" type="button" on:click={reloadTranscript} disabled={transcriptLoading}>
            {transcriptLoading ? 'Reloading' : 'Reload'}
          </button>
        </div>
      </div>

      {#if transcriptError}
        <div class="inline-empty compact-empty">
          <strong>Local transcript read failed</strong>
          <p>{transcriptError}</p>
        </div>
      {:else if transcriptResponse?.status !== 'available'}
        <div class="inline-empty compact-empty">
          <strong>No local Codex conversation link</strong>
          <p>{transcriptResponse?.reason ?? 'No local Codex transcript candidate was found.'}</p>
        </div>
      {:else}
        <div class="codex-link-summary">
          <button class="btn btn-primary" type="button" on:click={openCodexTranscript} disabled={!transcriptDeepLink}>
            Open in Codex
          </button>
          <div class="codex-link-times" aria-label="Codex conversation message times">
            <div>
              <span>Last sent</span>
              <strong>{formatTimeWithRelative(transcriptResponse?.lastUserMessageAt)}</strong>
            </div>
            <div>
              <span>Last returned</span>
              <strong>{formatTimeWithRelative(transcriptResponse?.lastAssistantMessageAt)}</strong>
            </div>
          </div>
          {#if transcriptResponse?.threadId}
            <code>{transcriptResponse.threadId}</code>
          {/if}
        </div>
      {/if}
    </section>

    <div class="lane-lifecycle-list">
      {#if issueTimelineLoading}
        <p class="section-note">Loading GitHub issue comments...</p>
      {:else if issueTimelineError}
        <p class="section-note">GitHub issue comments unavailable: {issueTimelineError}</p>
      {/if}
      {#if sourceLinkError}
        <p class="section-note">Source link failed: {sourceLinkError}</p>
      {/if}
      {#if lifecycleEvents.length}
        {#each lifecycleEvents as event}
          <article class="lane-lifecycle-row">
            <time>{formatTime(event.time)}</time>
            <div>
              <span>{event.phase}</span>
              <strong>{event.label}</strong>
              {#if event.detail}
                <p>{event.detail}</p>
              {/if}
            </div>
            {#if event.url}
              <button class="queue-link source-link-button" type="button" on:click={() => openSourceLink(event.url)}>Source</button>
            {/if}
          </article>
        {/each}
      {:else}
        <div class="inline-empty">
          <strong>No lifecycle events visible</strong>
          <p>The current readback does not include timeline evidence for this issue.</p>
        </div>
      {/if}
    </div>
  </section>
{:else}
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
                <a
                  class="lane-board-item {issue.runtimeTone} {issue.runtimeCategory === 'Active runtime' ? 'picked' : ''}"
                  href={issuePath(issue)}
                  on:click={(event) => navigate(event, issuePath(issue))}
                >
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

  <section class="lane-completed-panel" aria-label="Local issue worktrees">
    <div class="lane-completed-head">
      <div class="lane-completed-actions">
        <button class="btn btn-ghost" type="button" on:click={refreshLocalArtifacts} disabled={localArtifactsRefresh?.running}>
          {localArtifactsRefresh?.running ? 'Refreshing' : 'Refresh local'}
        </button>
        <div class="segmented-control compact" role="group" aria-label="Local worktree time window">
          {#each completedWindows as window}
            <button
              class:active={completedWindowHours === window.hours}
              type="button"
              on:click={() => setCompletedWindow(window.hours)}
            >
              {window.label}
            </button>
          {/each}
        </div>
      </div>
      {#if filteredCompletedLocalIssues.length}
        <div class="lane-completed-pagination">
          <span>{completedPageStart}-{completedPageEnd} of {filteredCompletedLocalIssues.length}</span>
          <div class="pagination">
            <button class="btn btn-ghost" type="button" on:click={previousCompletedPage} disabled={completedPage <= 1}>Previous</button>
            <span>Page {completedPage} of {completedPageCount}</span>
            <button class="btn btn-ghost" type="button" on:click={nextCompletedPage} disabled={completedPage >= completedPageCount}>Next</button>
          </div>
        </div>
      {/if}
    </div>

    <div class="lane-completed-list">
      {#if filteredCompletedLocalIssues.length}
        <div class="lane-completed-pagination">
          <span>{completedPageStart}-{completedPageEnd} of {filteredCompletedLocalIssues.length}</span>
          <div class="pagination">
            <button class="btn btn-ghost" type="button" on:click={previousCompletedPage} disabled={completedPage <= 1}>Previous</button>
            <span>Page {completedPage} of {completedPageCount}</span>
            <button class="btn btn-ghost" type="button" on:click={nextCompletedPage} disabled={completedPage >= completedPageCount}>Next</button>
          </div>
        </div>
        <div class="lane-completed-table-head" aria-hidden="true">
          <span>Issue</span>
          <span>Title</span>
          <span>Created</span>
          <span>Last Progress</span>
          <span>Last Modified</span>
          <span>Tree</span>
          <span>Branch</span>
          <span>Head</span>
        </div>
        {#each pagedCompletedLocalIssues as issue}
          <a
            class="lane-completed-row"
            href={issuePath(issue)}
            on:click={(event) => navigate(event, issuePath(issue))}
          >
            <span class="issue-tag">{issue.id}</span>
            <strong class="lane-completed-title">{issue.title}</strong>
            <span>{formatTime(issue.worktree?.createdAt)}</span>
            <span
              class:unknown={!progressAge(issue).known}
              class="lane-completed-age"
              title={progressAge(issue).title}
            >{progressAge(issue).label}</span>
            <span class="lane-completed-age">{relativeAge(issue.worktree?.lastModified)}</span>
            <span class:dirty={treeStateLabel(issue.worktree?.treeState) === 'Dirty'} class="lane-tree-state">{treeStateLabel(issue.worktree?.treeState)}</span>
            <code class="lane-completed-branch">{issue.worktree?.branch ?? 'branch unknown'}</code>
            <code class="lane-completed-headsha">{shortHead(issue.worktree?.head)}</code>
          </a>
        {/each}
      {:else}
        <div class="inline-empty compact-empty">
          <strong>No local issue worktree in this window</strong>
          <p>Switch to All or refresh after an issue worktree with a complete title is visible locally.</p>
        </div>
      {/if}
    </div>
  </section>
{/if}
