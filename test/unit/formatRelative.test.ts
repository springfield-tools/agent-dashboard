import { describe, expect, it } from 'vitest';

import { formatRelativeTime } from '../../src/formatRelative';

describe('formatRelativeTime', () => {
  const now = Date.parse('2026-10-10T20:00:00Z');

  it('returns just now for recent timestamps', () => {
    expect(formatRelativeTime('2026-10-10T19:59:30Z', now)).toBe('just now');
  });

  it('formats minutes and hours', () => {
    expect(formatRelativeTime('2026-10-10T19:45:00Z', now)).toBe('15m ago');
    expect(formatRelativeTime('2026-10-10T18:00:00Z', now)).toBe('2h ago');
  });

  it('formats days', () => {
    expect(formatRelativeTime('2026-10-08T20:00:00Z', now)).toBe('2d ago');
  });
});
