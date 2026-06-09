import type { GitHubUserSnapshot } from '../tauriAutoloop.ts';

export type ThemeMode = 'daylight' | 'night';
export type RefreshInterval = 'manual' | '10000' | '30000' | '60000';

export type NavigatorItem = {
  href: string;
  label: string;
};

export type RefreshOption = {
  value: RefreshInterval;
  label: string;
};

export type AutoloopControlSnapshot = {
  tauriAvailable: boolean;
  busy: boolean;
  running: boolean;
  mode: string;
  workflowPath: string;
  latestLine: string;
  laneMaxSummary: string;
};

export type NavigatorNavigateHandler = (event: MouseEvent, href: string) => void;

export type { GitHubUserSnapshot };
