<script lang="ts">
  import ReferencePanels from '$lib/ReferencePanels.svelte';

  const runbooks = [
    {
      skill: 'Manual Main',
      trigger: 'Todo, Main-lane Rework, or resumable In Progress',
      reads: ['project state', 'doctor', 'project inspect', 'project issue --json', 'forge validate'],
      produces: 'Implementation evidence, Main Agent Workpad, PR handoff to Agent Review',
      boundary: 'Does not approve, human-review, or merge.'
    },
    {
      skill: 'Manual Review',
      trigger: 'Agent Review work needing independent review evidence',
      reads: ['linked PR', 'diff and focused validation', 'Main Workpad', 'review freshness'],
      produces: 'Pass evidence, confirmed findings, or blocked review evidence',
      boundary: 'Does not mutate Main Workpad or act as Human Review.'
    },
    {
      skill: 'Human Review',
      trigger: 'Human Review issue with independent Review Agent pass evidence',
      reads: ['project issue --json', 'issue comments', 'linked PR readback', 'UAT checklist'],
      produces: 'Append-only human decision note and route to Merging or Rework',
      boundary: 'Requires explicit operator confirmation before Project mutation.'
    },
    {
      skill: 'Manual Merge',
      trigger: 'Merging issue or approved merge-lane recovery',
      reads: ['project issue --json', 'PR mergeability', 'Human Review evidence', 'status checks'],
      produces: 'Merge evidence, Project readback, cleanup result, Done or Need Human Input',
      boundary: 'Does not perform fresh Todo implementation.'
    },
    {
      skill: 'Doctor',
      trigger: 'Readiness blocker, stale metadata, missing evidence, or lane health gap',
      reads: ['doctor output', 'project issue readback', 'timeline evidence', 'local setup state'],
      produces: 'Repair recommendation and confirmed safe repairs when allowed',
      boundary: 'Does not replace lane ownership or hide unresolved human decisions.'
    }
  ];

  const routeRules = [
    { state: 'Todo', owner: 'Manual Main', action: 'Run quality gate before implementation.' },
    { state: 'Need to Clarify', owner: 'Issue Forge / operator', action: 'Ask the smallest execution-critical question.' },
    { state: 'Need Human Input', owner: 'Operator + relevant Skill', action: 'Record decision or missing external input before continuing.' },
    { state: 'Agent Review', owner: 'Manual Review', action: 'Review independently and record pass/finding evidence.' },
    { state: 'Human Review', owner: 'Human Review', action: 'Brief UAT and wait for explicit route confirmation.' },
    { state: 'Merging', owner: 'Manual Merge', action: 'Verify approval and land or route unsafe cases to Need Human Input.' }
  ];

  const runbookStats = [
    { label: 'Skills mapped', value: runbooks.length },
    { label: 'State routes', value: routeRules.length },
    { label: 'Human gates', value: routeRules.filter((rule) => rule.owner.includes('Human') || rule.state.includes('Human')).length }
  ];

  const skillHandoffs = [
    {
      name: 'Manual Main',
      lane: 'Todo / Rework',
      reads: 'Issue gate, workpad, workspace, linked PR',
      output: 'Implementation handoff to Agent Review'
    },
    {
      name: 'Manual Review',
      lane: 'Agent Review',
      reads: 'PR diff, focused validation, review freshness',
      output: 'Pass evidence or confirmed findings'
    },
    {
      name: 'Human Review',
      lane: 'Human Review',
      reads: 'Independent review evidence and UAT notes',
      output: 'Explicit approve-to-Merging or rework route'
    },
    {
      name: 'Manual Merge',
      lane: 'Merging',
      reads: 'Approval evidence, PR mergeability, cleanup status',
      output: 'Done readback or Need Human Input'
    }
  ];

  const boundaryCards = [
    {
      lane: 'Main',
      owns: 'Issue implementation, workpad, PR handoff',
      stops: 'Agent Review',
      evidence: 'Main Agent Workpad and linked PR readback'
    },
    {
      lane: 'Agent Review',
      owns: 'Independent findings and pass evidence',
      stops: 'Human Review or Rework',
      evidence: 'Review timeline comment and checklist evidence'
    },
    {
      lane: 'Human Review',
      owns: 'Operator UAT and route decision',
      stops: 'Merging or Rework',
      evidence: 'Human decision note and confirmed acceptance'
    },
    {
      lane: 'Merge',
      owns: 'Approved PR landing and cleanup readback',
      stops: 'Done or Need Human Input',
      evidence: 'Merge evidence, Project readback, cleanup result'
    }
  ];

  const timelineModel = [
    { lane: 'Main', writer: 'Main Agent', evidence: 'Owns the persistent Main Workpad and PR implementation evidence.' },
    { lane: 'Agent Review', writer: 'Review Agent', evidence: 'Adds independent pass or finding evidence without rewriting Main work.' },
    { lane: 'Human Review', writer: 'Operator', evidence: 'Records the explicit route decision after UAT.' },
    { lane: 'Merge', writer: 'Merging Agent', evidence: 'Records merge, cleanup, and final Project readback.' }
  ];
</script>

<section class="route-hero">
  <div>
    <p class="eyebrow">Operator Runbook</p>
    <h2>Skill Routing</h2>
    <p>
      Use this page as the bridge between the visual cockpit and chat-led Shea Symphony operations.
      The Web UI shows evidence; Skills remain the primary execution surface.
    </p>
  </div>
  <a class="btn btn-primary" href="/">Open desk</a>
</section>

<section class="summary-strip" aria-label="Runbook summary">
  {#each runbookStats as stat}
    <div>
      <strong>{stat.value}</strong>
      <span>{stat.label}</span>
    </div>
  {/each}
</section>

<section class="runbook-grid" aria-label="Shea Symphony skill runbooks">
  {#each runbooks as item}
    <article class="runbook-card">
      <div>
        <span class="mini-label">{item.trigger}</span>
        <h3>{item.skill}</h3>
      </div>
      <dl>
        <div>
          <dt>Reads</dt>
          <dd>{item.reads.join(' · ')}</dd>
        </div>
        <div>
          <dt>Produces</dt>
          <dd>{item.produces}</dd>
        </div>
        <div>
          <dt>Boundary</dt>
          <dd>{item.boundary}</dd>
        </div>
      </dl>
    </article>
  {/each}
</section>

<ReferencePanels {skillHandoffs} {boundaryCards} {timelineModel} />

<section class="routing-table" aria-labelledby="routing-table-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">State Routing</p>
      <h2 id="routing-table-title">What To Do Next</h2>
    </div>
    <span class="section-note">Reference only; route in chat after evidence review</span>
  </div>

  <div class="routing-grid">
    {#each routeRules as rule}
      <article>
        <span>{rule.state}</span>
        <strong>{rule.owner}</strong>
        <p>{rule.action}</p>
      </article>
    {/each}
  </div>
</section>
