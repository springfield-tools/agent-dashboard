export type PlaceholderGroup = {
  id: string;
  title: string;
  placeholderTitle: string;
};

/** Phase 1 placeholder sections; order matches status precedence in docs/PLAN.md. */
export const PLACEHOLDER_GROUPS: readonly PlaceholderGroup[] = [
  {
    id: 'needs-input',
    title: 'Needs input',
    placeholderTitle: 'Nothing waiting on you',
  },
  {
    id: 'ci-failing',
    title: 'CI failing',
    placeholderTitle: 'No failing checks',
  },
  {
    id: 'needs-testing',
    title: 'Needs testing',
    placeholderTitle: 'Nothing to test',
  },
  {
    id: 'ready-to-merge',
    title: 'Ready to merge',
    placeholderTitle: 'Nothing approved yet',
  },
  {
    id: 'agent-working',
    title: 'Agent working',
    placeholderTitle: 'No agents in flight',
  },
  {
    id: 'not-started',
    title: 'Not started',
    placeholderTitle: 'No unlabeled work',
  },
];

export function groupCount(): number {
  return PLACEHOLDER_GROUPS.length;
}
