export function textFromValue(value: any, fallback = '') {
  if (value == null || value === '' || value === 'none') return fallback;
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  if (typeof value === 'object') {
    return textFromValue(
      value.title ??
        value.name ??
        value.label ??
        value.action ??
        value.status ??
        value.state ??
        value.identifier ??
        value.issue,
      fallback
    );
  }
  return fallback;
}

export function firstLine(value: any) {
  return String(value).split('\n').find(Boolean) ?? String(value);
}

export function titleCase(value: any) {
  return String(value)
    .replace(/[-_]/g, ' ')
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
}

export function timeLabel(date: Date) {
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}
