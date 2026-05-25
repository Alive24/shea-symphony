export const attentionTasks = [
  {
    id: '#418',
    title: 'ProjectV2 metadata refresh needs a routing decision',
    type: 'Need Human Input',
    reason:
      'The tracker can refresh stale Status options once, but the next write needs an operator decision on whether to pause dispatch or continue with cached reads.',
    action: 'Record Decision',
    recommended:
      'Choose continue-with-readonly evidence for this run, then route the issue back to In Progress.',
    evidence:
      'REST fields returned a new Status option id after refresh; GraphQL fallback stayed available for rich issue data.',
    urgency: 'Decision needed',
    tone: 'warn',
    decisions: [
      {
        label: 'Continue read-only',
        result: 'Record operator decision, keep write-mode paused, and rerun Project readback.',
        writes: 'Runtime: dry-run only; Timeline: operator chose read-only recovery.'
      },
      {
        label: 'Retry refresh',
        result: 'Allow one metadata refresh before any Project mutation is attempted.',
        writes: 'Runtime: one refresh attempt; Timeline: retry authorized.'
      },
      {
        label: 'Defer',
        result: 'Leave issue parked in Need Human Input with the current evidence preview.',
        writes: 'Project Status: Need Human Input; Timeline: no mutation authorized.'
      }
    ]
  },
  {
    id: '#421',
    title: 'Agent Review timed out before Human Review evidence',
    type: 'Human Review',
    reason:
      'The review lane completed local checks but did not record a passing independent review before the timeout window closed.',
    action: 'Review PR',
    recommended:
      'Inspect the recorded review timeline, then approve to Merging or return confirmed findings to Rework.',
    evidence:
      'Review attempt wrote queued, running, and timed-out states; no confirmed findings were recorded.',
    urgency: 'Review blocked',
    tone: 'danger',
    decisions: [
      {
        label: 'Approve to Merging',
        result: 'Append Human Review decision evidence and move the issue to Merging.',
        writes: 'Timeline: Human Review approved; Project Status: Merging; Workpad: unchanged.'
      },
      {
        label: 'Request Rework',
        result: 'Record confirmed findings and route back to Rework without changing the Main Workpad.',
        writes: 'Timeline: confirmed findings; Project Status: Rework; Workpad: unchanged.'
      },
      {
        label: 'Defer',
        result: 'Keep Human Review parked and request fresher review evidence.',
        writes: 'Timeline: deferred for freshness; Project Status: Human Review; Workpad: unchanged.'
      }
    ]
  },
  {
    id: '#409',
    title: 'Issue Forge draft is missing dependency semantics',
    type: 'Need Human Input',
    reason:
      'The quality gate cannot dispatch an issue with placeholder dependency language because the agent may claim work out of order.',
    action: 'Open Issue',
    recommended:
      'Replace the dependency section with No blocking dependencies or an explicit Blocked By reference.',
    evidence:
      'Gate classification: Need to Clarify. Missing field: executable dependency status.',
    urgency: 'Clarify before dispatch',
    tone: 'warn',
    decisions: [
      {
        label: 'Open Issue Forge',
        result: 'Ask for dependency semantics before dispatch.',
        writes: 'Issue Forge: clarification question; Project Status: Need Human Input.'
      },
      {
        label: 'Mark no blockers',
        result: 'Record No blocking dependencies and return to Main dispatch.',
        writes: 'Issue body: dependency field updated; Project Status: Todo.'
      },
      {
        label: 'Defer',
        result: 'Keep the draft parked until dependency ownership is known.',
        writes: 'Project Status: Need Human Input; Timeline: dependency owner unknown.'
      }
    ]
  }
];

export const laneSummaries = [
  {
    name: 'Main',
    href: '/lanes/main',
    active: 3,
    retrying: 1,
    blocked: 1,
    latest: 'Quality Gate paused #409 for dependency clarification',
    posture: 'dispatch'
  },
  {
    name: 'Review',
    href: '/lanes/review',
    active: 2,
    retrying: 0,
    blocked: 1,
    latest: 'Independent review timeout recorded for #421',
    posture: 'review'
  },
  {
    name: 'Merge',
    href: '/lanes/merge',
    active: 1,
    retrying: 0,
    blocked: 0,
    latest: 'PR freshness check passed for #397',
    posture: 'merge'
  }
];

export const readinessItems = [
  { label: 'Canonical checkout', status: 'Ready', tone: 'success' },
  { label: 'Doctor', status: 'Pass', tone: 'success' },
  { label: 'Auth', status: 'Refresh soon', tone: 'warn' },
  { label: 'Backend health', status: 'Healthy', tone: 'success' }
];

export const recentEvents = [
  {
    time: '14:18',
    lane: 'Review',
    title: 'Agent Review timeline appended',
    detail: 'Review attempt for #421 ended without Human Review routing evidence.'
  },
  {
    time: '14:12',
    lane: 'Main',
    title: 'Quality Gate classified #409',
    detail: 'Dependency language was incomplete; issue stayed out of dispatch.'
  },
  {
    time: '14:08',
    lane: 'Merge',
    title: 'PR freshness gate passed',
    detail: 'Linked pull request evidence matched the Project issue readback.'
  }
];

const laneWorkerActions = {
  main: [
    'Refresh tracker and worktree state',
    'Run Issue Quality Gate',
    'Update Main Agent Workpad',
    'Render backend prompt',
    'Record lane timeline comment',
    'Verify linked PR visibility',
    'Repair stale Project metadata',
    'Check assignee dispatch eligibility',
    'Reconcile terminal workspace',
    'Hydrate linked pull requests',
    'Capture decision assumptions',
    'Run doctor preflight',
    'Prepare Agent Review handoff',
    'Confirm target state transition',
    'Sync project item readback',
    'Rerun focused validation',
    'Park issue for human input'
  ],
  review: [
    'Read linked pull request and diff summary',
    'Check review-freshness evidence',
    'Run focused validation command',
    'Classify findings',
    'Record pass evidence',
    'Record confirmed finding',
    'Append review timeline comment',
    'Verify Main Workpad remains unchanged',
    'Check parent/subissue routing rule',
    'Prepare Human Review briefing',
    'Move routine native subissue to Merging',
    'Route confirmed finding to Rework'
  ],
  merge: [
    'Read approved Human Review evidence',
    'Verify PR mergeability',
    'Refresh merge worktree',
    'Repair mechanical drift',
    'Run post-repair validation',
    'Append merge-lane evidence',
    'Merge clean PR',
    'Read back Project status',
    'Check local cleanup result',
    'Route unsafe conflict to Need Human Input'
  ]
};

const laneConfig = {
  main: {
    title: 'Tracker adapter evidence path',
    alternate: 'Workflow handoff policy check',
    backendA: 'Codex app-server',
    backendB: 'Claude Code',
    targets: ['Agent Review', 'Need Human Input', 'In Progress', 'Rework'],
    evidenceA: 'Workpad marker found; Project item read through REST-first path.',
    evidenceB: 'Timeline comment includes lane, actor, target state, and evidence summary.'
  },
  review: {
    title: 'Independent review evidence gate',
    alternate: 'Human Review routing check',
    backendA: 'Review Agent',
    backendB: 'Codex app-server',
    targets: ['Human Review', 'Rework', 'Merging', 'Agent Review'],
    evidenceA: 'Linked PR and focused validation are fresh enough for operator review.',
    evidenceB: 'Findings summary is append-only; Main Agent Workpad remains untouched.'
  },
  merge: {
    title: 'Approved PR landing path',
    alternate: 'Merge-lane repair boundary',
    backendA: 'Merge Agent',
    backendB: 'Local git runner',
    targets: ['Merged', 'Need Human Input', 'Merging', 'Rework'],
    evidenceA: 'PR readback matches Human Review approval and Project item linkage.',
    evidenceB: 'Mechanical repair evidence is lane-local and recorded on the timeline.'
  }
};

function buildWorkers(lane) {
  const config = laneConfig[lane];
  return laneWorkerActions[lane].map((action, index) => {
  const issueNumber = 430 + index;
  const elapsed = index % 3 === 0 ? '07m' : index % 3 === 1 ? '18m' : '31m';
  const backend = index % 2 === 0 ? config.backendA : config.backendB;
  const target = config.targets[index % config.targets.length];

  return {
    issue: `#${issueNumber}`,
    title: index % 2 === 0 ? config.title : config.alternate,
    action,
    backend,
    session: `${lane}-${String(index + 1).padStart(2, '0')}`,
    elapsed,
    evidence: index % 2 === 0 ? config.evidenceA : config.evidenceB,
    target
  };
});
}

export const laneWorkers = {
  main: buildWorkers('main'),
  review: buildWorkers('review'),
  merge: buildWorkers('merge')
};

export const mainWorkers = laneWorkers.main;

export const fullEvents = [
  ...recentEvents,
  {
    time: '13:58',
    lane: 'Main',
    title: 'Assignee filter accepted #418',
    detail: 'Issue was assigned to an allowed owner and had a matching ProjectV2 item.'
  },
  {
    time: '13:44',
    lane: 'System',
    title: 'Doctor check completed',
    detail: 'Canonical checkout, auth, and backend readiness were readable.'
  },
  {
    time: '13:31',
    lane: 'Merge',
    title: 'Merge lane stayed separate from Human Review',
    detail: 'Approved work moved through the merge lane without rewriting Main Agent Workpad.'
  }
];
