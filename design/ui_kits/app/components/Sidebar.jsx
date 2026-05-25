function Sidebar({ activeNav, onNav }) {
  const nav = ["Operator Desk", "Lanes", "Events", "Settings"];
  return (
    <header className="ss-navbar" aria-label="Shea Symphony navigation">
      <div className="ss-brand">
        <img src="../../build/favicon.svg" alt="Shea Symphony" />
        <div>
          <strong>Shea Symphony</strong>
          <small>Human operator cockpit</small>
        </div>
      </div>
      <nav className="ss-nav">
        {nav.map((item) => (
          <button key={item} className={activeNav === item ? "active" : ""} onClick={() => onNav(item)}>
            {item}
          </button>
        ))}
      </nav>
      <div className="ss-navbar-status" aria-label="Runtime status">
        <span className="ss-pill">Foreground Autopilot</span>
        <span className="ss-pill">Write guarded</span>
        <span className="ss-pill">Healthy</span>
      </div>
    </header>
  );
}

window.Sidebar = Sidebar;
