import { describe, expect, it } from 'vitest';

import { teamLabel } from './workspace';

describe('team label', () => {
  it('names a team by its key and its name', () => {
    expect(teamLabel({ key: 'ENG', name: 'Engineering' })).toBe('ENG · Engineering');
  });
});
