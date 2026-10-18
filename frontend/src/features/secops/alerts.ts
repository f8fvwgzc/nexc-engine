import type { Alert } from '@/schemas/policy';

/** Unacknowledged first, then by time, newest first. */
export function orderAlerts(alerts: Alert[]): Alert[] {
  return [...alerts].sort((a, b) => {
    const open = Number(a.acknowledged_at !== null) - Number(b.acknowledged_at !== null);
    return open || b.created_at.localeCompare(a.created_at);
  });
}
