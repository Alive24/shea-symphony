export type HumanTodoRefreshState = {
  badge: string;
  title: string;
  detail: string;
  status: 'loading' | 'refreshing' | 'manual' | 'unavailable' | 'empty';
  isClear: boolean;
};

type HumanTodoRefreshInput = {
  visibleIssueCount?: number;
  fullLoading?: boolean;
  slowReadsRemaining?: number;
  operatorSurfaceRefreshing?: boolean;
  liveUnavailable?: boolean;
  hasProjectQueueRead?: boolean;
};

export function humanTodoRefreshState(input: HumanTodoRefreshInput): HumanTodoRefreshState {
  const visibleIssueCount = Number(input.visibleIssueCount ?? 0);
  if (visibleIssueCount > 0) {
    return {
      badge: 'Action',
      title: 'Human to-do issues visible',
      detail: `${visibleIssueCount} operator-owned issue${visibleIssueCount === 1 ? '' : 's'} visible.`,
      status: 'empty',
      isClear: false
    };
  }

  const remaining = Math.max(0, Number(input.slowReadsRemaining ?? 0));
  if (input.fullLoading) {
    return {
      badge: 'Loading',
      title: 'Checking human to-do issues',
      detail: `Loading CLI readback... ${remaining} surface${remaining === 1 ? '' : 's'} remaining.`,
      status: 'loading',
      isClear: false
    };
  }

  if (input.operatorSurfaceRefreshing) {
    return {
      badge: 'Refreshing',
      title: 'Refreshing human to-do issues',
      detail: 'Waiting for refreshed Project readback before showing operator-owned issues.',
      status: 'refreshing',
      isClear: false
    };
  }

  if (input.liveUnavailable) {
    return {
      badge: 'Unavailable',
      title: 'Live readback unavailable',
      detail: 'Waiting for live Project readback before showing operator-owned issues.',
      status: 'unavailable',
      isClear: false
    };
  }

  if (!input.hasProjectQueueRead) {
    return {
      badge: 'Refresh',
      title: 'Refresh needed',
      detail: 'Refresh the operator queue before treating Human Todo as clear.',
      status: 'manual',
      isClear: false
    };
  }

  return {
    badge: 'Clear',
    title: 'No human to-do issues visible',
    detail: 'The current Project read did not surface Need to Clarify, Need Human Input, or Human Review items.',
    status: 'empty',
    isClear: true
  };
}
