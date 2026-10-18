import type { RiskPoint } from '@/schemas/secops';

/** Days the risk chart covers. */
export const RISK_DAYS = 14;

/** One day of the chart: the date and a score per agent that was scored that day. */
export interface RiskRow {
  day: string;
  [agentId: string]: number | string;
}

/** `YYYY-MM-DD` of `at` in UTC. */
export function dayOf(at: Date): string {
  return at.toISOString().slice(0, 10);
}

/** The last `days` days ending today (UTC), oldest first. */
export function recentDays(days = RISK_DAYS, now = new Date()): string[] {
  const out: string[] = [];
  for (let i = days - 1; i >= 0; i -= 1) {
    out.push(dayOf(new Date(now.getTime() - i * 86_400_000)));
  }
  return out;
}

/**
 * Lays the overview's risk history out for a line chart: one row per day of the window, a
 * column per agent that was scored at least once. Days without a score for an agent carry no
 * value, so its line breaks rather than reading as zero.
 */
export function riskSeries(
  history: RiskPoint[],
  days = RISK_DAYS,
  now = new Date(),
): { rows: RiskRow[]; agents: string[] } {
  const window = recentDays(days, now);
  const agents = [...new Set(history.map((p) => p.agent_id))];
  const byDay = new Map<string, RiskRow>(window.map((day) => [day, { day }]));
  for (const point of history) {
    const row = byDay.get(point.day);
    if (!row) continue;
    const current = row[point.agent_id];
    row[point.agent_id] =
      typeof current === 'number' ? Math.max(current, point.score) : point.score;
  }
  return { rows: [...byDay.values()], agents };
}

/** `Oct 3` for the chart's axis. */
export function shortDay(day: string): string {
  const date = new Date(`${day}T00:00:00Z`);
  return new Intl.DateTimeFormat('en', { month: 'short', day: 'numeric', timeZone: 'UTC' }).format(
    date,
  );
}
