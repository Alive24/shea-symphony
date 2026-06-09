import { titleCase } from './text.ts';

export function buildParkedTasks(autopilot: any, githubQueue: any, githubQueueResult: any) {
  const autopilotTasks = (autopilot?.parked_queues ?? []).flatMap((queue) =>
    (queue.issues ?? []).map((issue) =>
      parkedTaskFromIssue({
        id: issue.identifier ?? issue.issue ?? queue.state ?? 'Issue',
        title: issue.title ?? `${queue.state ?? 'Parked'} queue item`,
        state: queue.state ?? queue.queue ?? 'Parked',
        reason: issue.reason ?? queue.reason ?? 'Issue is parked outside active lane dispatch.',
        recommended: queue.next_action ?? issue.next_action ?? 'Inspect the issue readback before routing.',
        evidence: issue.evidence ?? queue.evidence ?? 'Autoloop plan surfaced this item.',
        assignees: issue.assignees ?? [],
        source: 'Autoloop plan'
      })
    )
  );
  if (autopilotTasks.length) return autopilotTasks;

  if (githubQueue?.operatorIssues?.length) {
    return githubQueue.operatorIssues.map((issue) =>
      parkedTaskFromIssue({
        id: issue.identifier,
        title: issue.title,
        state: issue.state,
        reason: `${issue.state} issue is visible in the operator queue readback.`,
        recommended:
          issue.state === 'Human Review'
            ? 'Review evidence in chat Skill before routing.'
            : 'Inspect diagnostics and issue readback before routing.',
        evidence: `${githubQueue.source ?? 'Operator queue'} · updated ${issue.updatedAt ?? 'unknown'}`,
        assignees: issue.assignees ?? [],
        source: 'Operator queue readback'
      })
    );
  }

  if (githubQueueResult?.ok && githubQueue?.totalOpen != null) return [];
  return [];
}

export function laneSourceFor(lanePlan: any, autopilotResult: any, overview: any, githubQueueResult: any, githubQueue: any) {
  if (overview?.fixture === true) {
    return {
      provenance: 'fixture',
      sourceLabel: lanePlan ? 'Fixture autoloop' : 'Fixture fallback',
      sourceTone: 'warn',
      countsReliable: true
    };
  }

  if (lanePlan) {
    return {
      provenance: 'live',
      sourceLabel: 'Live autoloop',
      sourceTone: 'success',
      countsReliable: true
    };
  }

  if (githubQueue?.laneCounts && githubQueueResult?.ok) {
    return {
      provenance: 'live',
      sourceLabel: 'Live GitHub queue',
      sourceTone: 'success',
      countsReliable: true
    };
  }

  if (autopilotResult?.ok) {
    return {
      provenance: 'live',
      sourceLabel: 'Live empty lane',
      sourceTone: 'success',
      countsReliable: true
    };
  }

  if (autopilotResult) {
    if (autopilotResult.pending) {
      return {
        provenance: 'partial',
        sourceLabel: 'Pending slow read',
        sourceTone: 'warn',
        countsReliable: false
      };
    }
    return {
      provenance: autopilotResult.timedOut ? 'partial' : 'fallback',
      sourceLabel: autopilotResult.timedOut ? 'Timed-out fallback' : 'Fallback posture',
      sourceTone: autopilotResult.timedOut ? 'warn' : 'danger',
      countsReliable: false
    };
  }

  return {
    provenance: 'fallback',
    sourceLabel: 'Layout fallback',
    sourceTone: 'danger',
    countsReliable: false
  };
}

export function countLaneStatus(autopilot: any, lane: string, status: string) {
  return (autopilot?.lane_activity ?? []).filter((item) => item.lane === lane && item.status === status)
    .length;
}

export function eventRowsFromAutopilot(autopilot: any) {
  return (autopilot?.lanes ?? []).map((lane) => ({
    time: 'live',
    lane: titleCase(lane.lane ?? 'lane'),
    title: `${titleCase(lane.lane ?? 'Lane')} ${lane.status ?? 'status'}`,
    detail: `${lane.action ?? 'No action'}: ${lane.reason ?? 'No reason supplied.'}`
  }));
}

export function buildEvidenceColumns(events: any[]) {
  return ['System', 'Main', 'Review', 'Merge', 'Human Review']
    .map((lane) => ({
      lane,
      events: events.filter((event) => event.lane === lane).slice(0, 3)
    }))
    .filter((column) => column.events.length > 0);
}

function parkedTaskFromIssue({ id, title, state, reason, recommended, evidence, assignees = [], source }) {
  return {
    id,
    title,
    type: state,
    reason,
    action: 'Inspect Issue',
    recommended,
    evidence,
    urgency: state,
    tone: state === 'Need Human Input' ? 'danger' : 'warn',
    assignees,
    sourceLabel: source,
    decisions: [
      {
        label: 'Open readback',
        result: 'Read project issue JSON and linked PR evidence.',
        writes: 'Read-only command.',
        commandAction: 'project-issue'
      },
      {
        label: 'Quality gate',
        result: 'Run the issue quality gate in dry-run mode.',
        writes: 'Dry-run command.',
        commandAction: 'quality-gate'
      }
    ]
  };
}
