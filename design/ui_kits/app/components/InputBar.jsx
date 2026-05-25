function InputBar({ onSubmit }) {
  const [value, setValue] = React.useState("confirm approve to Merging");
  function submitDecision(event) {
    event.preventDefault();
    if (!value.trim()) return;
    onSubmit(value.trim());
    setValue("");
  }
  return (
    <form className="ss-inputbar" onSubmit={submitDecision}>
      <textarea
        aria-label="Operator decision"
        value={value}
        onChange={(event) => setValue(event.target.value)}
        placeholder="Record a literal routing decision..."
      />
      <button className="ss-button ss-button-compact" type="submit">Record</button>
    </form>
  );
}

window.InputBar = InputBar;
