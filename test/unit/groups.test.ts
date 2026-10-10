import { describe, expect, it } from 'vitest';

/** Phase 2 display order matches docs/PLAN.md precedence list. */
const GROUP_ORDER = [
  'needs-input',
  'ci-failing',
  'needs-testing',
  'ready-to-merge',
  'agent-working',
  'not-started',
];

describe('group order', () => {
  it('lists six sections in precedence order', () => {
    expect(GROUP_ORDER).toHaveLength(6);
    expect(GROUP_ORDER[0]).toBe('needs-input');
    expect(GROUP_ORDER[5]).toBe('not-started');
  });
});
