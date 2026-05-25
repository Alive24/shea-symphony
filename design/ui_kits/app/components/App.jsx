function App() {
  const [activeNav, setActiveNav] = React.useState("Operator Desk");
  const [messages, setMessages] = React.useState([
    {
      role: "Review Agent",
      time: "14:12 CST",
      text: "Review freshness passed. PR linkage is visible through project issue --json and the Agent Review pass evidence is recorded."
    },
    {
      role: "Human Operator",
      time: "14:14 CST",
      text: "Need one safe action exposed: approve to Merging or send back to Rework with a concrete blocker."
    }
  ]);

  const lanes = [
    { posture: "Main", name: "Main lane", active: 2, retrying: 1, blocked: 0, latest: "Issue #398 handed off to Agent Review with PR and validation evidence." },
    { posture: "Review", name: "Review lane", active: 1, retrying: 0, blocked: 1, latest: "Issue #396 passed review freshness and is waiting for Human Review." },
    { posture: "Merge", name: "Merge lane", active: 1, retrying: 1, blocked: 0, latest: "Merge lane is retrying a transient UNKNOWN mergeability read." }
  ];

  const events = [
    { time: "14:17:03", title: "Doctor", detail: "No missing PR handoff evidence for #396." },
    { time: "14:15:42", title: "Review", detail: "Agent Review pass timeline comment recorded." },
    { time: "14:13:18", title: "Project", detail: "Human Review state confirmed through tracker readback." }
  ];

  function recordDecision(text) {
    setMessages((current) => current.concat({
      role: "Human Operator",
      time: "now",
      text: text
    }));
  }

  return (
    <div className="ss-app">
      <Sidebar activeNav={activeNav} onNav={setActiveNav} />
      <main className="ss-workspace">
        <header className="ss-topbar">
          <div>
            <span className="ss-mini-label">{activeNav}</span>
            <h1>Human Operator Desk</h1>
          </div>
          <div className="ss-cluster">
            <span className="ss-pill">Canonical ready</span>
            <span className="ss-pill">Doctor ready</span>
            <span className="ss-pill">Auth ready</span>
            <span className="ss-pill" style={{ background: "color-mix(in oklab, var(--ss-success), transparent 82%)", color: "var(--ss-success)", border: "1px solid color-mix(in oklab, var(--ss-success), transparent 70%)" }}>Healthy</span>
          </div>
        </header>
        <div className="ss-main-grid">
          <section className="ss-panel pad">
            <div className="ss-section-head">
              <div>
                <span className="ss-mini-label">Needs human attention</span>
                <h2>Top operator decisions</h2>
              </div>
              <span className="ss-pill">3 recent events</span>
            </div>
            <div className="ss-attention-stack">
              <article className="ss-attention-card warn">
                <div className="ss-attention-topline">
                  <span className="ss-issue">#396</span>
                  <span className="ss-pill">Human Review</span>
                </div>
                <div className="ss-attention-body">
                  <span className="ss-mini-label">Review freshness repair complete</span>
                  <h3>Approve to Merging after reading the recorded review pass.</h3>
                  <div className="ss-evidence">
                    <span>Latest evidence</span>
                    <p>project issue --json confirms PR linkage; Agent Review evidence is present; no Main-lane mutation required.</p>
                  </div>
                </div>
                <button className="ss-button" type="button">Approve</button>
              </article>
              <article className="ss-attention-card danger">
                <div className="ss-attention-topline">
                  <span className="ss-issue">#398</span>
                  <span className="ss-pill">Rework</span>
                </div>
                <div className="ss-attention-body">
                  <span className="ss-mini-label">Stale conflicted PR</span>
                  <h3>Send back to Rework with the merge conflict as the concrete blocker.</h3>
                  <div className="ss-evidence">
                    <span>Latest evidence</span>
                    <p>Doctor surfaced parent-topology noise, but the decisive blocker is stale PR reality.</p>
                  </div>
                </div>
                <button className="ss-button ss-button-secondary" type="button">Route Rework</button>
              </article>
            </div>
          </section>
          <aside className="ss-side-stack">
            <section className="ss-panel pad">
              <div className="ss-section-head">
                <div>
                  <span className="ss-mini-label">Lane summaries</span>
                  <h2>Workers</h2>
                </div>
              </div>
              <AssistantsList lanes={lanes} />
            </section>
            <ChatArea messages={messages} onSubmit={recordDecision} />
            <section className="ss-panel pad">
              <div className="ss-section-head">
                <div>
                  <span className="ss-mini-label">Recent events</span>
                  <h2>Evidence</h2>
                </div>
              </div>
              <div className="ss-message-list">
                {events.map((event) => (
                  <div className="ss-event" key={`${event.time}-${event.title}`}>
                    <span>{event.time} / {event.title}</span>
                    <strong>{event.detail}</strong>
                  </div>
                ))}
              </div>
            </section>
          </aside>
        </div>
      </main>
    </div>
  );
}

window.App = App;
