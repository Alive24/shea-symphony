type LooseRecord = Record<string, any>;

export function buildGateChecklist() {
  return [
    {
      label: 'Issue Quality Gate',
      status: 'Before dispatch',
      detail: 'Todo and Rework items must prove goal, scope, dependencies, guardrails, and verification.'
    },
    {
      label: 'Agent Review Gate',
      status: 'Before Human Review',
      detail: 'Main lane stops at Agent Review; independent review records pass evidence or findings.'
    },
    {
      label: 'Human Decision Gate',
      status: 'Before Merging',
      detail: 'Human Review needs explicit approval or confirmed rework routing evidence.'
    },
    {
      label: 'Merge Readback Gate',
      status: 'Before Done',
      detail: 'Merge lane verifies PR landing, Project state readback, and cleanup evidence.'
    }
  ];
}

export function buildTimelineModel() {
  return [
    {
      lane: 'Main',
      writer: 'Persistent Workpad',
      evidence: 'Context, plan, work log, validation, PR handoff, and rework rounds.'
    },
    {
      lane: 'Review',
      writer: 'Append-only timeline',
      evidence: 'Queued/running/completed review state, finding classification, and supported checklist evidence.'
    },
    {
      lane: 'Human',
      writer: 'Decision note',
      evidence: 'Operator UAT result and literal approve-to-Merging or rework decision.'
    },
    {
      lane: 'Merge',
      writer: 'Append-only timeline',
      evidence: 'Mergeability, repair evidence, merge result, Project readback, and cleanup status.'
    }
  ];
}

export function buildCapabilityMap(commands: LooseRecord) {
  return [
    {
      label: 'Tracker client abstraction',
      state: commands.autopilot?.ok ? 'Observed' : 'Pending read',
      tone: commands.autopilot?.ok ? 'success' : 'warn'
    },
    {
      label: 'Independent review',
      state: commands.review?.ok ? 'Observed' : 'Pending read',
      tone: commands.review?.ok ? 'success' : 'warn'
    },
    {
      label: 'Doctor diagnostics',
      state: commands.doctor?.ok ? 'Observed' : 'Pending read',
      tone: commands.doctor?.ok ? 'success' : 'warn'
    },
    {
      label: 'Status',
      state: commands.status?.ok ? 'Observed' : 'Pending read',
      tone: commands.status?.ok ? 'success' : 'warn'
    },
    {
      label: 'Operator queue',
      state: commands.githubQueue?.ok ? 'Observed' : 'Pending read',
      tone: commands.githubQueue?.ok ? 'success' : 'warn'
    }
  ];
}
