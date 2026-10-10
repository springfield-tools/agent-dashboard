import { describe, expect, it } from 'vitest';

import { groupCount, PLACEHOLDER_GROUPS } from '../../src/groups';

describe('groups', () => {
  it('lists six status sections in plan order', () => {
    expect(groupCount()).toBe(6);
    expect(PLACEHOLDER_GROUPS.map((g) => g.id)).toEqual([
      'needs-input',
      'ci-failing',
      'needs-testing',
      'ready-to-merge',
      'agent-working',
      'not-started',
    ]);
  });
});
