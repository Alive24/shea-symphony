<script lang="ts">
  import CommandHealthPanel from './lib/CommandHealthPanel.svelte';
  import DataSourcePanel from './lib/DataSourcePanel.svelte';
  import EvidenceColumns from './lib/EvidenceColumns.svelte';
  import IntelligenceDashboard from './lib/IntelligenceDashboard.svelte';
  import IssueIndex from './lib/IssueIndex.svelte';
  import LaneIssueView from './lib/LaneIssueView.svelte';
  import OperatorBrief from './lib/OperatorBrief.svelte';
  import ReadPathMap from './lib/ReadPathMap.svelte';
  import ReadSurfaceObservatory from './lib/ReadSurfaceObservatory.svelte';
  import ReferencePanels from './lib/ReferencePanels.svelte';
  import RuntimeRibbon from './lib/RuntimeRibbon.svelte';
  import WorkflowMap from './lib/WorkflowMap.svelte';

  export let route = '/intelligence';
  export let view: any;

  const skillHandoffs = [
    { lane: 'Main', name: 'shea-symphony-manual-main', reads: 'Project issue, workpad, linked PR', output: 'Agent Review handoff' },
    { lane: 'Review', name: 'shea-symphony-manual-review', reads: 'PR diff, tests, review evidence', output: 'Human Review or Rework' },
    { lane: 'Merge', name: 'shea-symphony-manual-merge', reads: 'Approved PR, freshness, CI', output: 'Done or merge-lane repair' }
  ];

  const boundaryCards = [
    { lane: 'Main', owns: 'Implementation and Main-lane Rework', stops: 'Agent Review', evidence: 'Workpad plus PR handoff' },
    { lane: 'Review', owns: 'Independent review and routing recommendation', stops: 'Human Review or Rework', evidence: 'Review note and test/UAT readback' },
    { lane: 'Merge', owns: 'Approved PR landing', stops: 'Done', evidence: 'Freshness, merge result, closeout' }
  ];

  $: diagnosticCount = view?.attentionTasks?.filter((task) => task.type === 'Diagnostics').length ?? 0;
  $: blockedCount = view?.laneSummaries?.reduce((total, lane) => total + Number(lane.blocked ?? 0), 0) ?? 0;
  $: stateMax = Math.max(1, ...(view?.stateDistribution ?? []).map((row) => Number(row.count ?? 0)));
  $: stateTiles = [
    { label: 'Attention', value: view?.attentionTasks?.length ?? 0, tone: (view?.attentionTasks?.length ?? 0) ? 'warn' : 'success' },
    { label: 'Active lanes', value: view?.laneSummaries?.reduce((total, lane) => total + Number(lane.active ?? 0), 0) ?? 0, tone: 'neutral' },
    { label: 'Blocked', value: blockedCount, tone: blockedCount ? 'danger' : 'success' },
    { label: 'Indexed', value: view?.issueIndex?.length ?? 0, tone: 'neutral' }
  ];
  $: visualPosture = [
    ...((view?.laneSummaries ?? []).map((lane) => ({
      role: lane.name,
      name: lane.posture ?? 'unknown',
      detail: lane.latest ?? 'No live lane data.',
      status: lane.blocked ? 'blocked' : lane.active ? 'active' : 'idle',
      lane
    }))),
    { role: 'Finish', name: 'Done', detail: 'Merged work leaves the active cockpit.', status: 'idle' }
  ];

  function titleCase(value: string) {
    return String(value ?? '')
      .replace(/[-_]/g, ' ')
      .replace(/\b\w/g, (letter) => letter.toUpperCase());
  }
</script>

{#if route.startsWith('/lanes')}
  <LaneIssueView {view} {route} />
{:else if route === '/doctor' || route === '/observability'}
  <RuntimeRibbon
    source={view?.dataSource}
    generatedAtLabel={view?.generatedAtLabel}
    healthy={view?.healthy}
    fixture={view?.fixture}
    attentionCount={view?.attentionTasks?.length ?? 0}
    diagnosticCount={diagnosticCount}
    blockedCount={blockedCount}
  />
  <DataSourcePanel source={view?.dataSource} />
  <ReadSurfaceObservatory commands={view?.commandHealth ?? []} />
  <CommandHealthPanel commands={view?.commandHealth ?? []} />
  <ReadPathMap paths={view?.readPathMap ?? []} />
{:else if route === '/intelligence' || route === '/reference'}
  <IntelligenceDashboard
    trackerSignals={view?.trackerSignals ?? []}
    gateChecklist={view?.gateChecklist ?? []}
    capabilityMap={view?.capabilityMap ?? []}
  />
  <WorkflowMap
    visualPosture={visualPosture}
    stateTiles={stateTiles}
    stateDistribution={view?.stateDistribution ?? []}
    stateMax={stateMax}
    fixture={view?.fixture}
  />
  <EvidenceColumns
    title="Cross-Lane Signals"
    eyebrow="Evidence Flow"
    columns={view?.evidenceColumns ?? []}
    href="/doctor"
  />
  <ReferencePanels
    skillHandoffs={skillHandoffs}
    boundaryCards={boundaryCards}
    timelineModel={view?.timelineModel ?? []}
  />
{:else}
  <OperatorBrief brief={view?.operatorBrief} />
  <IssueIndex issues={view?.issueIndex ?? []} limit={12} />
{/if}
