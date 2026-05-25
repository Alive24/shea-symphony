<script lang="ts">
  export let task: any;
  export let onOpen: (task: any) => void = () => {};
</script>

<article class="attention-card {task.tone}">
  <div class="attention-topline">
    <span class="issue-tag">{task.id}</span>
    <span class="status-pill {task.tone}">{task.urgency}</span>
  </div>

  <div class="attention-body">
    <div>
      <p class="attention-type">{task.type}</p>
      <h3>{task.title}</h3>
    </div>
    <p>{task.reason}</p>
  </div>

  <div class="recommendation">
    <span>Recommended next action</span>
    <strong>{task.recommended}</strong>
  </div>

  <div class="evidence-preview">
    <span>Evidence preview</span>
    <p>{task.evidence}</p>
  </div>

  <button class="btn btn-primary" type="button" on:click={() => onOpen(task)}>{task.action}</button>
</article>
