function AssistantsList({ lanes }) {
  return (
    <section className="ss-lane-list" aria-label="Lane summaries">
      {lanes.map((lane) => (
        <article className="ss-lane-card" key={lane.name}>
          <div className="ss-lane-head">
            <div>
              <span className="ss-mini-label">{lane.posture}</span>
              <h3>{lane.name}</h3>
            </div>
            <button className="ss-button ss-button-secondary ss-button-compact" type="button">View lane</button>
          </div>
          <div className="ss-metrics" aria-label={`${lane.name} worker summary`}>
            <div className="ss-metric"><strong>{lane.active}</strong><span>active</span></div>
            <div className="ss-metric"><strong>{lane.retrying}</strong><span>retrying</span></div>
            <div className="ss-metric"><strong>{lane.blocked}</strong><span>blocked</span></div>
          </div>
          <p className="ss-card-note">{lane.latest}</p>
        </article>
      ))}
    </section>
  );
}

window.AssistantsList = AssistantsList;
