import { buildIssueIndex } from './issueIndex.ts';
import { buildEvidenceColumns } from './laneActivity.ts';
import { buildOperatorBrief } from './operatorBrief.ts';
import { buildProjectWorkerMatch, buildWorkerMonitor } from './workerRuntime.ts';
import { buildFallbackTrackerSignals } from './readSurfaces.ts';
import { buildCommandHealth, buildReadPathMap } from './commandHealth.ts';
import { buildStateDistribution } from './stateDistribution.ts';
import { buildCapabilityMap, buildGateChecklist, buildTimelineModel } from './workflowModels.ts';

export function fallbackViewModel(reason: string) {
  const offlineTask = {
    id: 'local-api',
    title: 'Live data unavailable',
    type: 'Diagnostics',
    reason,
    action: 'Start local server',
    recommended: 'Start or reconnect the local Shea web server before trusting Project or worker status.',
    evidence: reason,
    urgency: 'Offline',
    tone: 'danger'
  };
  const offlineLaneSummaries = ['Main', 'Review', 'Merge'].map((name) => ({
    name,
    href: `/lanes/${name.toLowerCase()}`,
    active: 0,
    retrying: 0,
    blocked: 0,
    latest: 'Live API unavailable; no Project or worker count is trusted.',
    posture: 'unknown',
    provenance: 'fallback',
    sourceLabel: 'Live API unavailable',
    sourceTone: 'danger'
  }));
  const offlineEvents = [
    {
      time: 'now',
      lane: 'System',
      title: 'Live data unavailable',
      detail: reason
    }
  ];
  const offlineDataSource = {
    mode: 'offline',
    label: 'Live data unavailable',
    tone: 'danger',
    freshness: 'No live API response',
    trust: 'Do not trust Project or worker counts yet',
    detail: reason
  };

  return {
    attentionTasks: [offlineTask],
    laneSummaries: offlineLaneSummaries,
    laneWorkers: null,
    laneProjectIssues: { main: [], review: [], merge: [] },
    projectWorkerMatch: buildProjectWorkerMatch(
      { main: [], review: [], merge: [] },
      { main: [], review: [], merge: [] },
      { status: 'unknown' }
    ),
    workerMonitor: buildWorkerMonitor([], null, { main: [], review: [], merge: [] }, { status: 'unknown' }),
    currentFocus: null,
    sessionReadState: { status: 'unknown', detail: 'Session surface unavailable while local API is offline.' },
    sessionWorkers: [],
    readinessItems: [
      { label: 'Local API', status: 'Offline', tone: 'danger' },
      { label: 'Project queue', status: 'Not checked', tone: 'warn' },
      { label: 'Worker sessions', status: 'Not checked', tone: 'warn' },
      { label: 'Diagnostics', status: 'Not checked', tone: 'warn' }
    ],
    recentEvents: offlineEvents,
    fullEvents: offlineEvents,
    stateDistribution: buildStateDistribution(null, offlineLaneSummaries, []),
    evidenceColumns: buildEvidenceColumns(offlineEvents),
    trackerSignals: buildFallbackTrackerSignals(reason),
    gateChecklist: buildGateChecklist(),
    timelineModel: buildTimelineModel(),
    capabilityMap: buildCapabilityMap({}),
    dataSource: offlineDataSource,
    queueIssues: [],
    issueIndex: buildIssueIndex(
      [],
      {
        main: [],
        review: [],
        merge: []
      },
      offlineEvents,
      []
    ),
    commandHealth: buildCommandHealth({}),
    readPathMap: buildReadPathMap(buildCommandHealth({})),
    operatorBrief: buildOperatorBrief({
      attentionTasks: [offlineTask],
      laneSummaries: offlineLaneSummaries,
      evidenceColumns: buildEvidenceColumns(offlineEvents),
      dataSource: offlineDataSource
    }),
    generatedAtLabel: 'offline',
    healthy: false,
    fixture: false,
    workflowPath: 'workflows/shea-symphony.md',
    raw: null
  };
}
