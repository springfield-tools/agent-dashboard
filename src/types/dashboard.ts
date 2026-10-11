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
  graphqlPollCost?: number | null;
  graphqlRemaining?: number | null;
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
  credentialSource: string;
  authError: string | null;
  offline: boolean;
  banner: string | null;
};

/** Matches `DEFAULT_POLL_INTERVAL` in `src-tauri/src/config.rs`. */
export const DEFAULT_POLL_INTERVAL_SECONDS = 60;

export type RowActionKind =
  | 'testingPassedReady'
  | 'testingPassedHandBack'
  | 'sendBackToAgent'
  | 'answer'
  | 'approveReadyToMerge';

export type RowActionPreview = {
  action: RowActionKind;
  labelsToAdd: string[];
  labelsToRemove: string[];
  commentBody: string;
  wakesAgent: boolean;
  noteRequired: boolean;
  defaultNote: string;
};

export type WriteUiState = {
  pendingKeys: string[];
  undoAvailable: boolean;
  undoMessage: string | null;
  partialFailure: {
    kind: 'retryComment' | 'retryLabels';
    message: string;
  } | null;
  toast: string | null;
};

export const POLL_INTERVAL_PRESETS = [
  { label: '15s', seconds: 15 },
  { label: '30s', seconds: 30 },
  { label: '1m', seconds: 60 },
  { label: '2m', seconds: 120 },
] as const;
