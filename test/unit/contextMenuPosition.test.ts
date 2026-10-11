import { describe, expect, it } from 'vitest';

import {
  computeContextMenuPosition,
  estimateContextMenuHeight,
  rowMenuEntries,
} from '../../src/rowActionMenu';

describe('computeContextMenuPosition', () => {
  const popover = { left: 100, top: 50, width: 320, height: 400 } as DOMRectReadOnly;

  it('places menu at click when there is room', () => {
    const pos = computeContextMenuPosition(popover, 180, 120, { width: 160, height: 80 });
    expect(pos.left).toBe(80);
    expect(pos.top).toBe(70);
  });

  it('flips left when menu would overflow the right edge', () => {
    const pos = computeContextMenuPosition(popover, 480, 120, { width: 160, height: 80 });
    expect(pos.left).toBe(154);
    expect(pos.top).toBe(70);
  });

  it('flips up when menu would overflow the bottom', () => {
    const pos = computeContextMenuPosition(popover, 180, 420, { width: 160, height: 80 });
    expect(pos.left).toBe(80);
    expect(pos.top).toBe(290);
  });

  it('clamps inside padding when popover is tight', () => {
    const tiny = { left: 0, top: 0, width: 100, height: 60 } as DOMRectReadOnly;
    const pos = computeContextMenuPosition(tiny, 90, 55, { width: 120, height: 80 });
    expect(pos.left).toBe(6);
    expect(pos.top).toBe(6);
  });
});

describe('estimateContextMenuHeight', () => {
  it('grows with item count', () => {
    expect(estimateContextMenuHeight(1)).toBeLessThan(estimateContextMenuHeight(3));
  });
});

describe('rowMenuEntries', () => {
  it('offers a single action for needs-input without a cancel affordance in the menu layer', () => {
    const entries = rowMenuEntries('needs-input');
    expect(entries).toHaveLength(1);
    expect(entries[0]?.mode).toBe('answer');
  });

  it('offers testing passed choices for needs-testing', () => {
    expect(rowMenuEntries('needs-testing')).toHaveLength(2);
  });
});
