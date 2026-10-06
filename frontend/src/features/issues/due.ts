const DAY_MS = 24 * 60 * 60 * 1000;
const dayFormat = new Intl.DateTimeFormat('en', {
  month: 'short',
  day: 'numeric',
  timeZone: 'UTC',
});

/** Today as `YYYY-MM-DD` in the viewer's own time zone: a due date is a calendar day there. */
export function localToday(now = new Date()): string {
  const month = String(now.getMonth() + 1).padStart(2, '0');
  const day = String(now.getDate()).padStart(2, '0');
  return `${now.getFullYear()}-${month}-${day}`;
}

/** Whole days from `today` to `due` (both `YYYY-MM-DD`); negative when it has passed. */
export function daysUntil(due: string, today: string): number {
  return Math.round((Date.parse(`${due}T00:00:00Z`) - Date.parse(`${today}T00:00:00Z`)) / DAY_MS);
}

/**
 * How a due date reads in a list: the day, or how near it is. `late` is for work that is still
 * open; a closed issue is never late.
 */
export function dueLabel(
  due: string,
  today: string,
  open: boolean,
): { text: string; late: boolean } {
  const days = daysUntil(due, today);
  if (open && days < 0) {
    return { text: `${-days} ${days === -1 ? 'day' : 'days'} late`, late: true };
  }
  if (open && days === 0) return { text: 'Due today', late: false };
  if (open && days === 1) return { text: 'Due tomorrow', late: false };
  return { text: dayFormat.format(new Date(`${due}T00:00:00Z`)), late: false };
}
