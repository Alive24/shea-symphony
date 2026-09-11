export type IssueDetail = {
  id: string;
  state?: string;
  worktree?: Record<string, unknown>;
  [key: string]: unknown;
};

/** Tracker readback owns lifecycle state; local completion only supplies artifacts. */
export function mergeIssueDetail(live: IssueDetail | undefined, local: IssueDetail | undefined): IssueDetail | undefined {
  if (!live) return local;
  return { ...local, ...live, worktree: { ...local?.worktree, ...live.worktree } };
}
