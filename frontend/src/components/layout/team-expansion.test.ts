import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  FOLDED_TEAMS_KEY,
  readFoldedTeams,
  withTeamOpen,
  writeFoldedTeams,
} from './team-expansion';

describe('the sidebar teams that are folded', () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => vi.restoreAllMocks());

  it('starts with every team unfolded', () => {
    expect(readFoldedTeams()).toEqual([]);
  });

  it('remembers what was folded', () => {
    writeFoldedTeams(['eng', 'des']);
    expect(localStorage.getItem(FOLDED_TEAMS_KEY)).toBe('["eng","des"]');
    expect(readFoldedTeams()).toEqual(['eng', 'des']);
  });

  it('folds and unfolds one team without touching the others', () => {
    expect(withTeamOpen([], 'eng', false)).toEqual(['eng']);
    expect(withTeamOpen(['eng'], 'eng', false)).toEqual(['eng']);
    expect(withTeamOpen(['eng', 'des'], 'eng', true)).toEqual(['des']);
    expect(withTeamOpen(['des'], 'eng', true)).toEqual(['des']);
  });

  it('ignores a garbled value', () => {
    localStorage.setItem(FOLDED_TEAMS_KEY, '{not json');
    expect(readFoldedTeams()).toEqual([]);
    localStorage.setItem(FOLDED_TEAMS_KEY, '{"eng":true}');
    expect(readFoldedTeams()).toEqual([]);
    localStorage.setItem(FOLDED_TEAMS_KEY, '["eng",3,null]');
    expect(readFoldedTeams()).toEqual(['eng']);
  });

  it('carries on when storage is unavailable', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('SecurityError');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('QuotaExceededError');
    });
    expect(readFoldedTeams()).toEqual([]);
    expect(() => writeFoldedTeams(['eng'])).not.toThrow();
  });
});
