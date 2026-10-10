export type ItemKind = 'issue' | 'pr';

export type RowChip = {
  label: string;
};

export type CiSnapshot = {
  state: string | null;
  failedChecks: string[];
};

export type DashboardRow = {
  kind: ItemKind;
  draft: boolean;
  org: string;
  repo: string;
  number: number;
  title: string;
  url: string;
  author: string;
  updatedAt: string;
  labels: string[];
  ci: CiSnapshot | null;
  chips: RowChip[];
};

export type DashboardGroup = {
  id: string;
  title: string;
  items: DashboardRow[];
};

export type BotFooter = {
  mode: string;
  green: number;
  noCi: number;
  failing: number;
  pending: number;
  searchUrl: string;
};

export type DashboardSnapshot = {
  schemaVersion: number;
  fetchedAt: string;
  stale: boolean;
  groups: DashboardGroup[];
  bots: BotFooter | null;
  trayTitle: string;
  trayTooltip: string;
};

export type ConfigStatus = {
  configPath: string;
  configError: string | null;
  pollIntervalSeconds: number;
  botFilter: string;
  fixtureMode: boolean;
  pollingPaused: boolean;
  pauseTooltip: string;
  hasGithubToken: boolean;
  authError: string | null;
  offline: boolean;
  banner: string | null;
};

export const POLL_INTERVAL_PRESETS = [
  { label: '15s', seconds: 15 },
  { label: '30s', seconds: 30 },
  { label: '1m', seconds: 60 },
  { label: '2m', seconds: 120 },
] as const;
