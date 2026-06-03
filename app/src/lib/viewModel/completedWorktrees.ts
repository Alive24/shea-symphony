export type RelativeAgeFormatter = (value: unknown) => string;

export function completedProgressDisplay(issue: any, relativeAge: RelativeAgeFormatter) {
  const source = issue?.worktree?.lastProgressSource ??
    issue?.lastProgressSource ??
    issue?.worktree?.timestampSources?.lastProgress?.source ??
    issue?.timestampSources?.lastProgress?.source ??
    'unavailable';
  const value = issue?.worktree?.lastProgressAt ?? issue?.lastProgressAt;
  if (!value || source === 'unavailable') {
    return {
      label: 'Unknown',
      title: 'No durable handoff progress evidence is visible locally.',
      source,
      known: false
    };
  }
  return {
    label: relativeAge(value),
    title: `Progress source: ${source}`,
    source,
    known: true
  };
}
