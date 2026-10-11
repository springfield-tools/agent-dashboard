import type { RowActionKind } from './types/dashboard';

export type MenuSize = { width: number; height: number };

export type RowMenuEntry = { mode: RowActionKind | 'testingPassed'; label: string };

export function rowMenuEntries(groupId: string): RowMenuEntry[] {
  switch (groupId) {
    case 'needs-testing':
      return [
        { mode: 'testingPassed', label: 'Testing passed…' },
        { mode: 'sendBackToAgent', label: 'Send back to agent' },
      ];
    case 'ci-failing':
      return [{ mode: 'sendBackToAgent', label: 'Send back to agent' }];
    case 'needs-input':
      return [{ mode: 'answer', label: 'Answer' }];
    case 'not-started':
    case 'agent-working':
      return [{ mode: 'approveReadyToMerge', label: 'Approve / ready to merge' }];
    default:
      return [];
  }
}

/** Position a context menu inside the popover, flipping to stay visible. */
export function computeContextMenuPosition(
  popoverRect: DOMRectReadOnly,
  clientX: number,
  clientY: number,
  menu: MenuSize,
  padding = 6,
): { left: number; top: number } {
  const relX = clientX - popoverRect.left;
  const relY = clientY - popoverRect.top;
  const maxLeft = Math.max(padding, popoverRect.width - menu.width - padding);
  const maxTop = Math.max(padding, popoverRect.height - menu.height - padding);
  let left = relX;
  let top = relY;
  if (left + menu.width > popoverRect.width - padding) {
    left = relX - menu.width;
  }
  if (top + menu.height > popoverRect.height - padding) {
    top = relY - menu.height;
  }
  left = Math.min(Math.max(left, padding), maxLeft);
  top = Math.min(Math.max(top, padding), maxTop);
  return { left, top };
}

/** Estimated menu height for flip calculations before layout. */
export function estimateContextMenuHeight(itemCount: number): number {
  const rowHeight = 28;
  const chrome = 8;
  return chrome + itemCount * rowHeight;
}
