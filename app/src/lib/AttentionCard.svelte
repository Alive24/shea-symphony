<script lang="ts">
  export let issue: any;
  export let disabled = false;
  export let handoffTargetLabel = 'Codex App';
  export let copied = false;
  export let message = '';
  export let onOpen: (issue: any) => void = () => {};
  export let onCopy: (issue: any) => void = () => {};

  function assigneeLabel(value: any) {
    const assignees = Array.isArray(value?.assignees) ? value.assignees.filter(Boolean) : [];
    if (!assignees.length) return 'Unassigned';
    if (assignees.length === 1) return assignees[0];
    return `${assignees[0]} +${assignees.length - 1}`;
  }
</script>

<article class="human-todo-card {issue.categoryTone}" class:refreshing={issue.refreshing}>
  <div class="human-todo-card-head">
    <div class="human-todo-identity">
      <span class="issue-tag">{issue.id}</span>
      <span class="assignee-pill">{assigneeLabel(issue)}</span>
    </div>
    <span class="human-todo-type {issue.categoryTone}">{issue.category}</span>
  </div>

  <div>
    <strong>{issue.title}</strong>
    <p>{issue.categoryDetail}</p>
  </div>

  <div class="human-todo-meta">
    <span>{issue.lane} · {issue.workerStatus}</span>
    <small>{issue.recommended}</small>
  </div>

  <div class="handoff-actions">
    <button class="btn btn-primary" type="button" disabled={disabled} onclick={() => onOpen(issue)}>
      Open in {handoffTargetLabel}
    </button>
    <button class="btn btn-ghost" type="button" disabled={disabled} onclick={() => onCopy(issue)}>
      {copied ? 'Copied' : 'Copy Handoff Prompt'}
    </button>
  </div>

  {#if message}
    <small class="handoff-status">{message}</small>
  {/if}
</article>
