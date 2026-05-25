<script lang="ts">
  import { onMount } from 'svelte';
  import CommandHealthPanel from '$lib/CommandHealthPanel.svelte';
  import DataSourcePanel from '$lib/DataSourcePanel.svelte';
  import HealthPanel from '$lib/HealthPanel.svelte';
  import { DATA_MODE_CHANGE_EVENT, buildViewModel, loadHealth, loadOverview } from '$lib/api';

  let view = buildViewModel(null);
  let loading = true;
  let error = '';
  let health = null;
  let healthError = '';

  async function refresh() {
    loading = true;
    error = '';
    healthError = '';
    try {
      const [overviewResult, healthResult] = await Promise.allSettled([loadOverview(false, 'fast'), loadHealth()]);
      if (overviewResult.status === 'fulfilled') {
        view = buildViewModel(overviewResult.value);
      } else {
        error = overviewResult.reason.message;
        view = buildViewModel(null);
      }
      if (healthResult.status === 'fulfilled') {
        health = healthResult.value;
      } else {
        healthError = healthResult.reason.message;
      }
    } catch (cause) {
      error = cause.message;
      view = buildViewModel(null);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    refresh();
    const dataModeListener = () => refresh();
    window.addEventListener(DATA_MODE_CHANGE_EVENT, dataModeListener);
    return () => window.removeEventListener(DATA_MODE_CHANGE_EVENT, dataModeListener);
  });
</script>

<section class="route-hero">
  <div>
    <p class="eyebrow">Local cockpit</p>
    <h2>Settings</h2>
    <p>Read-only observation posture for the local Shea Symphony cockpit.</p>
  </div>
  <span class="status-pill success">Observation only</span>
</section>

<section class="settings-status-grid">
  <DataSourcePanel source={view.dataSource} />
  <HealthPanel {health} error={healthError} />

  <section class="settings-card">
    <h3>Runtime evidence</h3>
    <div class="status-list">
      <div class="status-row">
        <span>Overview refresh</span>
        <span class="status-pill {loading ? 'warn' : error ? 'danger' : 'success'}">
          {loading ? 'Refreshing' : error ? 'Offline' : 'Readable'}
        </span>
      </div>
      <div class="status-row">
        <span>Workflow path</span>
        <span class="status-pill neutral">{view.workflowPath}</span>
      </div>
      <div class="status-row">
        <span>Write posture</span>
        <span class="status-pill warn">Chat Skills only</span>
      </div>
    </div>
  </section>
</section>

<CommandHealthPanel commands={view.commandHealth ?? []} />

<section class="settings-grid">
  <section class="settings-card">
    <h3>Observation cadence</h3>
    <div class="status-list">
      <div class="status-row">
        <span>Fast live read</span>
        <span class="status-pill success">45s</span>
      </div>
      <div class="status-row">
        <span>Pages covered</span>
        <span class="status-pill neutral">Desk / Lanes / Events</span>
      </div>
    </div>
    <p>Auto-read refreshes Project queue and worker session signals. Slow diagnostics stay behind manual refresh or Diagnostics.</p>
  </section>

  <section class="settings-card">
    <h3>Workflow</h3>
    <div class="status-list">
      <div class="status-row">
        <span>Workflow file</span>
        <span class="status-pill neutral">{view.workflowPath}</span>
      </div>
      <div class="status-row">
        <span>Override</span>
        <span class="status-pill neutral">SHEA_WORKFLOW</span>
      </div>
    </div>
    <p>The bundled server reads `SHEA_WORKFLOW`; restart with that variable to change the live workflow.</p>
  </section>

  <section class="settings-card">
    <h3>Action boundary</h3>
    <div class="status-list">
      <div class="status-row">
        <span>Web UI</span>
        <span class="status-pill success">Observe</span>
      </div>
      <div class="status-row">
        <span>Operations</span>
        <span class="status-pill warn">Chat Skills</span>
      </div>
    </div>
    <p>The cockpit visualizes live state. Lane mutations and routing decisions stay in the Shea Symphony Skills workflow.</p>
  </section>

  <section class="settings-card wide">
    <h3>Tracker authority</h3>
    <div class="status-list">
      <div class="status-row">
        <span>Workflow state source</span>
        <span class="status-pill neutral">ProjectV2 Status</span>
      </div>
      <div class="status-row">
        <span>Workpad source</span>
        <span class="status-pill neutral">Issue comment marker</span>
      </div>
      <div class="status-row">
        <span>Human Review authority</span>
        <span class="status-pill warn">Review lane only</span>
      </div>
    </div>
  </section>
</section>
