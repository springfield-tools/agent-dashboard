import type {
  DashboardGroup,
  DashboardOrgSection,
  DashboardRow,
  DashboardSnapshot,
  ItemKind,
} from './types/dashboard';

export type ItemKindFilter = 'all' | 'issue' | 'pr';

const TRAY_GROUP_IDS = ['needs-input', 'needs-testing', 'ci-failing', 'ready-to-merge'] as const;

export function parseItemKindFilter(value: string | undefined | null): ItemKindFilter {
  if (value === 'issue' || value === 'pr') {
    return value;
  }
  return 'all';
}

export function rowMatchesKindFilter(kind: ItemKind, filter: ItemKindFilter): boolean {
  if (filter === 'all') {
    return true;
  }
  return kind === filter;
}

function filterRows(rows: DashboardRow[], filter: ItemKindFilter): DashboardRow[] {
  return rows.filter((row) => rowMatchesKindFilter(row.kind, filter));
}

function filterGroups(groups: DashboardGroup[], filter: ItemKindFilter): DashboardGroup[] {
  const out: DashboardGroup[] = [];
  for (const group of groups) {
    const items = filterRows(group.items, filter);
    if (items.length > 0) {
      out.push({ ...group, items });
    }
  }
  return out;
}

/** Tray-style `input·testing·failing·merge` from visible groups (zeros omitted entirely). */
export function trayTitleFromGroups(groups: DashboardGroup[]): string {
  const parts = TRAY_GROUP_IDS.map((id) => {
    const group = groups.find((g) => g.id === id);
    return group?.items.length ?? 0;
  });
  if (parts.every((n) => n === 0)) {
    return '';
  }
  return parts.map(String).join('·');
}

function filterOrgSection(section: DashboardOrgSection, filter: ItemKindFilter): DashboardOrgSection {
  const groups = filterGroups(section.groups, filter);
  return {
    ...section,
    groups,
    trayTitle: trayTitleFromGroups(groups),
  };
}

export type FilteredDashboardView = {
  orgSections: DashboardOrgSection[];
  groups: DashboardGroup[];
  /** Dependabot footer is PR-only; hide when filtering to issues. */
  showBots: boolean;
};

export function filterDashboardView(
  snapshot: DashboardSnapshot,
  filter: ItemKindFilter,
): FilteredDashboardView {
  const showBots = filter !== 'issue';

  const orgSections = (snapshot.orgSections ?? [])
    .map((section) => filterOrgSection(section, filter))
    .filter((section) => section.groups.length > 0 || section.authBanner);

  const groups = filterGroups(snapshot.groups, filter);

  return { orgSections, groups, showBots };
}
