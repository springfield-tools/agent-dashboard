import { describe, expect, it } from 'vitest';

import {
  filterDashboardView,
  rowMatchesKindFilter,
  trayTitleFromGroups,
} from '../../src/typeFilter';
import type { DashboardRow, DashboardSnapshot } from '../../src/types/dashboard';

function makeRow(kind: 'issue' | 'pr'): DashboardRow {
  return {
    kind,
    draft: false,
    org: 'springfield-ham-radio',
    repo: 'r',
    number: 1,
    title: 't',
    url: 'https://example.com',
    author: 'a',
    updatedAt: '2026-10-10T12:00:00Z',
    labels: [],
    ci: null,
    chips: [],
  };
}

function snapshotWithRow(kind: 'issue' | 'pr', groupId: string): DashboardSnapshot {
  const item = makeRow(kind);
  const group = { id: groupId, title: groupId, items: [item] };
  return {
    schemaVersion: 2,
    fetchedAt: 'now',
    stale: false,
    trayTitle: '1·0·0·0',
    trayTooltip: 't',
    bots: {
      mode: 'collapse',
      green: 5,
      noCi: 0,
      failing: 0,
      pending: 0,
      searchUrl: 'https://x',
    },
    groups: [group],
    orgSections: [
      {
        org: 'springfield-ham-radio',
        trayTitle: '1·0·0·0',
        groups: [group],
      },
    ],
  };
}

describe('typeFilter', () => {
  it('matches row kinds', () => {
    expect(rowMatchesKindFilter('pr', 'all')).toBe(true);
    expect(rowMatchesKindFilter('issue', 'pr')).toBe(false);
    expect(rowMatchesKindFilter('issue', 'issue')).toBe(true);
  });

  it('recomputes org tray counts for filtered rows', () => {
    const snap = snapshotWithRow('pr', 'needs-input');
    const issuesOnly = filterDashboardView(snap, 'issue');
    expect(issuesOnly.orgSections).toHaveLength(0);

    const prsOnly = filterDashboardView(snap, 'pr');
    expect(prsOnly.orgSections[0]?.trayTitle).toBe('1·0·0·0');
  });

  it('hides Dependabot footer when showing issues only', () => {
    const snap = snapshotWithRow('pr', 'not-started');
    expect(filterDashboardView(snap, 'all').showBots).toBe(true);
    expect(filterDashboardView(snap, 'pr').showBots).toBe(true);
    expect(filterDashboardView(snap, 'issue').showBots).toBe(false);
  });

  it('formats tray title from group counts', () => {
    const groups = [
      { id: 'needs-input', title: 'Needs input', items: [makeRow('issue')] },
      { id: 'needs-testing', title: 'Needs testing', items: [] },
      {
        id: 'ci-failing',
        title: 'CI failing',
        items: [makeRow('pr'), makeRow('pr')],
      },
      { id: 'ready-to-merge', title: 'Ready', items: [] },
    ];
    expect(trayTitleFromGroups(groups)).toBe('1·0·2·0');
  });
});
