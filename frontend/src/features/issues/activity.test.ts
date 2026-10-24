import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { IssueEvent } from '@/schemas/issue';

import {
  changeText,
  DEFAULT_ACTIVITY_PREFS,
  readActivityPrefs,
  showsEvent,
  writeActivityPrefs,
} from './activity';

const KEY = 'nexc.issue.activity';

function event(kind: IssueEvent['kind'], fields: Partial<IssueEvent> = {}): IssueEvent {
  return {
    id: '01a10c67-ce89-7272-8865-1447210e01b4',
    issue_id: '6c08cc63-f95f-77d4-8576-23a9c41035ee',
    kind,
    actor: null,
    body: '',
    from: null,
    to: null,
    created_at: '2026-10-14T12:00:00Z',
    edited_at: null,
    run_id: null,
    ...fields,
  };
}

beforeEach(() => localStorage.clear());
afterEach(() => vi.restoreAllMocks());

describe('activity preferences', () => {
  it('start open, showing everything, oldest first', () => {
    expect(readActivityPrefs()).toEqual(DEFAULT_ACTIVITY_PREFS);
    expect(DEFAULT_ACTIVITY_PREFS).toEqual({ collapsed: false, show: 'all', order: 'asc' });
  });

  it('are remembered under one key', () => {
    writeActivityPrefs({ collapsed: true, show: 'files', order: 'desc' });
    expect(JSON.parse(localStorage.getItem(KEY) ?? '')).toEqual({
      collapsed: true,
      show: 'files',
      order: 'desc',
    });
    expect(readActivityPrefs()).toEqual({ collapsed: true, show: 'files', order: 'desc' });
  });

  it('fall back field by field on what they cannot read', () => {
    localStorage.setItem(KEY, JSON.stringify({ collapsed: 'yes', show: 'everything', order: 1 }));
    expect(readActivityPrefs()).toEqual(DEFAULT_ACTIVITY_PREFS);
    localStorage.setItem(KEY, '{not json');
    expect(readActivityPrefs()).toEqual(DEFAULT_ACTIVITY_PREFS);
    localStorage.setItem(KEY, JSON.stringify({ show: 'comments' }));
    expect(readActivityPrefs()).toEqual({ ...DEFAULT_ACTIVITY_PREFS, show: 'comments' });
  });

  it('work without storage (private mode, blocked site data)', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new DOMException('denied', 'SecurityError');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new DOMException('full', 'QuotaExceededError');
    });
    expect(readActivityPrefs()).toEqual(DEFAULT_ACTIVITY_PREFS);
    expect(() => writeActivityPrefs({ collapsed: true, show: 'runs', order: 'asc' })).not.toThrow();
  });
});

describe('what the activity shows', () => {
  const kinds = ['comment', 'state', 'priority', 'run', 'github', 'artifact'] as const;
  const shown = (show: Parameters<typeof showsEvent>[1]) =>
    kinds.filter((kind) => showsEvent(event(kind), show));

  it('splits comments, runs, files and every other change', () => {
    expect(shown('all')).toEqual(kinds);
    expect(shown('comments')).toEqual(['comment']);
    expect(shown('runs')).toEqual(['run']);
    expect(shown('files')).toEqual(['artifact']);
    expect(shown('changes')).toEqual(['state', 'priority', 'github']);
  });
});

describe('change entries', () => {
  it('say what happened to the GitHub link', () => {
    expect(changeText(event('github', { to: 'https://github.com/acme/platform/issues/12' }))).toBe(
      'linked GitHub issue acme/platform#12',
    );
    expect(changeText(event('github', { to: 'https://github.com/acme/platform/pull/7' }))).toBe(
      'linked GitHub pull request acme/platform#7',
    );
    expect(
      changeText(event('github', { from: 'https://github.com/acme/platform/issues/12' })),
    ).toBe('removed the GitHub link');
  });

  it('say what happened to a file', () => {
    const file = (to: string, from: string | null = null) =>
      changeText(event('artifact', { body: 'notes.md', from, to }));
    expect(file('uploaded')).toBe('uploaded notes.md');
    expect(file('updated:v3', '2')).toBe('updated notes.md to v3');
    expect(file('duplicated')).toBe('duplicated notes.md');
    expect(file('deleted', '3')).toBe('deleted notes.md');
  });
});
