import { describe, expect, it } from 'vitest';

import { daysUntil, dueLabel, localToday } from './due';

describe('due dates', () => {
  it('counts whole calendar days, across months', () => {
    expect(daysUntil('2026-11-02', '2026-10-30')).toBe(3);
    expect(daysUntil('2026-10-06', '2026-10-06')).toBe(0);
    expect(daysUntil('2026-09-30', '2026-10-02')).toBe(-2);
  });

  it('says how near an open issue is, and how late', () => {
    expect(dueLabel('2026-10-06', '2026-10-06', true)).toEqual({ text: 'Due today', late: false });
    expect(dueLabel('2026-10-07', '2026-10-06', true)).toEqual({
      text: 'Due tomorrow',
      late: false,
    });
    expect(dueLabel('2026-10-05', '2026-10-06', true)).toEqual({ text: '1 day late', late: true });
    expect(dueLabel('2026-10-01', '2026-10-06', true)).toEqual({ text: '5 days late', late: true });
    expect(dueLabel('2026-11-02', '2026-10-06', true)).toEqual({ text: 'Nov 2', late: false });
  });

  it('never calls a closed issue late', () => {
    expect(dueLabel('2026-10-01', '2026-10-06', false)).toEqual({ text: 'Oct 1', late: false });
  });

  it("reads today from the viewer's own calendar", () => {
    expect(localToday(new Date(2026, 0, 5, 23, 30))).toBe('2026-01-05');
  });
});
