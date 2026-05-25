<script lang="ts">
  export let trackerSignals = [];
  export let gateChecklist = [];
  export let capabilityMap = [];
</script>

<section class="intelligence-dashboard" aria-labelledby="intelligence-dashboard-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">Operating Contract</p>
      <h2 id="intelligence-dashboard-title">Tracker and Gate Intelligence</h2>
    </div>
    <span class="section-note">Derived from handoff policy and live command reads</span>
  </div>

  <div class="intelligence-grid">
    <section class="tracker-panel" aria-label="Tracker signals">
      <div>
        <p class="eyebrow">Tracker Signals</p>
        <h3>ProjectV2 Readiness</h3>
      </div>
      {#each trackerSignals as signal}
        <article class="signal-row {signal.tone}">
          <div>
            <strong>{signal.label}</strong>
            <span>{signal.status}</span>
          </div>
          <p>{signal.detail}</p>
        </article>
      {/each}
    </section>

    <section class="gate-panel" aria-label="Workflow gates">
      <div>
        <p class="eyebrow">Workflow Gates</p>
        <h3>Can This Continue?</h3>
      </div>
      {#each gateChecklist as gate}
        <article class="gate-row">
          <span>{gate.status}</span>
          <strong>{gate.label}</strong>
          <p>{gate.detail}</p>
        </article>
      {/each}
    </section>

    <section class="capability-panel" aria-label="Observability capabilities">
      <div>
        <p class="eyebrow">Observability</p>
        <h3>Capability Coverage</h3>
      </div>
      <div class="capability-grid">
        {#each capabilityMap as capability}
          <article class="capability-card {capability.tone}">
            <strong>{capability.state}</strong>
            <span>{capability.label}</span>
          </article>
        {/each}
      </div>
    </section>
  </div>
</section>
