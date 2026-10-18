import { describe, expect, it } from 'vitest';

import { recentDays, riskSeries, shortDay } from './risk';

const now = new Date('2026-10-18T09:30:00Z');
const a = '0190a1b2-c3d4-7e5f-8a9b-8c1d2e3f4a5b';
const b = '0190a1b2-c3d4-7e5f-8a9b-9c1d2e3f4a5b';

describe('riskSeries', () => {
  it('covers the window day by day with a column per agent, highest score per day', () => {
    const { rows, agents } = riskSeries(
      [
        { agent_id: a, day: '2026-10-17', score: 2.5 },
        { agent_id: a, day: '2026-10-17', score: 7 },
        { agent_id: b, day: '2026-10-18', score: 1 },
        { agent_id: a, day: '2026-09-01', score: 9 },
      ],
      3,
      now,
    );
    expect(agents).toEqual([a, b]);
    expect(rows.map((r) => r.day)).toEqual(['2026-10-16', '2026-10-17', '2026-10-18']);
    expect(rows[1]).toEqual({ day: '2026-10-17', [a]: 7 });
    expect(rows[2]).toEqual({ day: '2026-10-18', [b]: 1 });
    expect(rows[0]).toEqual({ day: '2026-10-16' });
  });

  it('names the days of the window and shortens them for the axis', () => {
    expect(recentDays(2, now)).toEqual(['2026-10-17', '2026-10-18']);
    expect(shortDay('2026-10-03')).toBe('Oct 3');
  });
});
