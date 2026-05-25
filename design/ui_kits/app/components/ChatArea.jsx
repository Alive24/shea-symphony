function ChatArea({ messages, onSubmit }) {
  return (
    <section className="ss-panel pad" aria-label="Human review workspace">
      <div className="ss-section-head">
        <div>
          <span className="ss-mini-label">Human Review</span>
          <h2>Decision ledger</h2>
        </div>
        <span className="ss-pill">Waiting on operator</span>
      </div>
      <div className="ss-message-list">
        {messages.map((message) => (
          <MessageBubble key={`${message.role}-${message.time}`} role={message.role} time={message.time}>
            {message.text}
          </MessageBubble>
        ))}
      </div>
      <InputBar onSubmit={onSubmit} />
    </section>
  );
}

window.ChatArea = ChatArea;
