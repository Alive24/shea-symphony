<script lang="ts">
  export let skillHandoffs = [];
  export let boundaryCards = [];
  export let timelineModel = [];
</script>

<section class="skill-dashboard" aria-labelledby="skill-dashboard-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">Skill Handoffs</p>
      <h2 id="skill-dashboard-title">Chat-Led Operations Map</h2>
    </div>
    <span class="section-note">Visual reference, not an action queue</span>
  </div>

  <div class="skill-grid">
    {#each skillHandoffs as skill}
      <article class="skill-card">
        <div>
          <span>{skill.lane}</span>
          <strong>{skill.name}</strong>
        </div>
        <dl>
          <div>
            <dt>Reads</dt>
            <dd>{skill.reads}</dd>
          </div>
          <div>
            <dt>Produces</dt>
            <dd>{skill.output}</dd>
          </div>
        </dl>
      </article>
    {/each}
  </div>
</section>

<section class="boundary-dashboard" aria-labelledby="boundary-dashboard-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">Lane Boundaries</p>
      <h2 id="boundary-dashboard-title">Responsibility Matrix</h2>
    </div>
    <span class="section-note">Read-only operator reference</span>
  </div>

  <div class="boundary-grid">
    {#each boundaryCards as card}
      <article class="boundary-card">
        <span>{card.lane}</span>
        <dl>
          <div>
            <dt>Owns</dt>
            <dd>{card.owns}</dd>
          </div>
          <div>
            <dt>Stops at</dt>
            <dd>{card.stops}</dd>
          </div>
          <div>
            <dt>Evidence</dt>
            <dd>{card.evidence}</dd>
          </div>
        </dl>
      </article>
    {/each}
  </div>
</section>

<section class="timeline-dashboard" aria-labelledby="timeline-dashboard-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">Evidence Timeline</p>
      <h2 id="timeline-dashboard-title">Who Writes What</h2>
    </div>
    <span class="section-note">Workpad stays Main-owned</span>
  </div>

  <div class="timeline-grid">
    {#each timelineModel as item}
      <article class="timeline-card">
        <span>{item.lane}</span>
        <strong>{item.writer}</strong>
        <p>{item.evidence}</p>
      </article>
    {/each}
  </div>
</section>
