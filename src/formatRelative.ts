const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/** Format an ISO timestamp as a short relative time for the popover row. */
export function formatRelativeTime(iso: string, nowMs: number = Date.now()): string {
  const then = Date.parse(iso);

  if (Number.isNaN(then)) {
    return '';
  }

  const delta = nowMs - then;

  if (delta < MINUTE_MS) {
    return 'just now';
  }

  if (delta < HOUR_MS) {
    const minutes = Math.floor(delta / MINUTE_MS);

    return `${minutes}m ago`;
  }

  if (delta < DAY_MS) {
    const hours = Math.floor(delta / HOUR_MS);

    return `${hours}h ago`;
  }

  const days = Math.floor(delta / DAY_MS);

  return `${days}d ago`;
}
