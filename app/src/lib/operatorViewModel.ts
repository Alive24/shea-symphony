import {
  fullEvents as fallbackEvents,
  laneSummaries as fallbackLaneSummaries
} from './data.ts';
import { fallbackViewModel } from './viewModel/fallbackViewModel.ts';
import {
  parseSessionReadState,
  parseSessionWorkers
} from './viewModel/sessionParsers.ts';
import {
  buildAutopilotQueueIssues,
  buildQueueIssues,
  mergeQueueIssues
} from './viewModel/queueIssues.ts';
import {
  annotateQueueIssuesWithWorkers,
  buildCurrentFocus,
  buildProjectWorkerMatch,
  buildWorkerMonitor,
  workersForLane
} from './viewModel/workerRuntime.ts';
import { buildEvidenceColumns, buildParkedTasks } from './viewModel/laneActivity.ts';
import { buildOperatorBrief } from './viewModel/operatorBrief.ts';
import {
  buildCommandFailures,
  buildLaneSummaries,
  buildReadinessItems,
  buildRecentEvents
} from './viewModel/overviewSections.ts';
import { buildDataSource, buildFallbackTrackerSignals, buildLiveSignals, buildTrackerSignals } from './viewModel/readSurfaces.ts';
import { buildCapabilityMap, buildGateChecklist, buildTimelineModel } from './viewModel/workflowModels.ts';
import { buildCommandHealth, buildReadPathMap } from './viewModel/commandHealth.ts';
import { buildStateDistribution } from './viewModel/stateDistribution.ts';
import { buildIssueIndex } from './viewModel/issueIndex.ts';
import { timeLabel } from './viewModel/text.ts';

type LooseRecord = Record<string, any>;

export function buildViewModel(overview: any): any {
  if (!overview) return fallbackViewModel('Waiting for Tauri or fixture readback.');

  const autopilot = overview.autopilot;
  const doctor = overview.doctor;
  const commands: LooseRecord = overview.commands ?? {};
  const githubQueue = overview.githubQueue;
  const generatedAt = overview.generatedAt ? new Date(overview.generatedAt) : null;
  const sessionReadState = parseSessionReadState(overview.sessionsText);
  const sessionWorkers = parseSessionWorkers(overview.sessionsText);
  const commandFailures = buildCommandFailures(commands);
  const parkedTasks = buildParkedTasks(autopilot, githubQueue, commands.githubQueue);
  const readinessItems = buildReadinessItems(commands, overview.targetContext);
  const laneSummaries = buildLaneSummaries({
    autopilot,
    githubQueue,
    commands,
    overview,
    fallbackLaneSummaries
  });

  const baseQueueIssues = mergeQueueIssues(
    buildQueueIssues(githubQueue, parkedTasks),
    buildAutopilotQueueIssues(autopilot)
  );
  const laneWorkers = {
    main: [...workersForLane(autopilot, 'main'), ...sessionWorkers.filter((worker) => worker.lane === 'main')],
    review: [...workersForLane(autopilot, 'review'), ...sessionWorkers.filter((worker) => worker.lane === 'review')],
    merge: [...workersForLane(autopilot, 'merge'), ...sessionWorkers.filter((worker) => worker.lane === 'merge')]
  };
  const queueIssues = annotateQueueIssuesWithWorkers(baseQueueIssues, laneWorkers, sessionReadState);
  const laneProjectIssues = {
    main: queueIssues.filter((issue) => issue.lane === 'Main'),
    review: queueIssues.filter((issue) => issue.lane === 'Review'),
    merge: queueIssues.filter((issue) => issue.lane === 'Merge')
  };
  const projectWorkerMatch = buildProjectWorkerMatch(laneProjectIssues, laneWorkers, sessionReadState);
  const workerMonitor = buildWorkerMonitor(sessionWorkers, autopilot, laneProjectIssues, sessionReadState);
  const currentFocus = buildCurrentFocus(queueIssues, projectWorkerMatch);

  const recentEvents = buildRecentEvents(autopilot, commands, generatedAt);
  const fullEvents = recentEvents.length ? recentEvents : fallbackEvents;
  const stateDistribution = buildStateDistribution(
    autopilot,
    laneSummaries,
    parkedTasks,
    githubQueue,
    commands.githubQueue,
    queueIssues
  );
  const evidenceColumns = buildEvidenceColumns(fullEvents);
  const trackerSignals = buildTrackerSignals(overview, commands);
  const gateChecklist = buildGateChecklist();
  const timelineModel = buildTimelineModel();
  const capabilityMap = buildCapabilityMap(commands);
  const dataSource = buildDataSource(overview, commands, generatedAt);
  const issueIndex = buildIssueIndex(parkedTasks, laneWorkers, fullEvents, queueIssues);
  const commandHealth = buildCommandHealth(commands);
  const readPathMap = buildReadPathMap(commandHealth);
  const liveSignals = buildLiveSignals(overview, commands);
  const operatorBrief = buildOperatorBrief({
    attentionTasks: [...parkedTasks, ...commandFailures],
    laneSummaries,
    evidenceColumns,
    dataSource
  });

  return {
    attentionTasks: [...parkedTasks, ...commandFailures].slice(0, 6),
    laneSummaries,
    laneWorkers,
    laneProjectIssues,
    projectWorkerMatch,
    workerMonitor,
    currentFocus,
    sessionReadState,
    sessionWorkers,
    readinessItems,
    recentEvents: recentEvents.length ? recentEvents : fallbackEvents.slice(0, 3),
    fullEvents,
    stateDistribution,
    evidenceColumns,
    trackerSignals,
    gateChecklist,
    timelineModel,
    capabilityMap,
    dataSource,
    queueIssues,
    issueIndex,
    commandHealth,
    readPathMap,
    liveSignals,
    operatorBrief,
    generatedAtLabel: generatedAt ? timeLabel(generatedAt) : 'not checked',
    healthy: overview.healthy,
    fixture: overview.fixture === true,
    workflowPath: overview.workflowPath,
    targetContext: overview.targetContext,
    raw: overview,
    doctor
  };
}
