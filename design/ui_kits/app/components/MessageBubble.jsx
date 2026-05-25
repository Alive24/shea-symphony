function MessageBubble({ role, time, children }) {
  return (
    <article className="ss-message">
      <header>
        <span>{role}</span>
        <span>{time}</span>
      </header>
      <p className="ss-card-note">{children}</p>
    </article>
  );
}

window.MessageBubble = MessageBubble;
