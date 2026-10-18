import { describe, expect, it } from 'vitest';

import { diffLines } from './diff';

describe('diffLines', () => {
  it('marks what a version added and removed against the one before', () => {
    const before = ['{', '  "a": 1,', '  "b": 2', '}'].join('\n');
    const after = ['{', '  "a": 1,', '  "b": 3,', '  "c": 4', '}'].join('\n');
    const kinds = diffLines(before, after).map((l) => `${l.kind}:${l.text.trim()}`);
    expect(kinds).toEqual([
      'same:{',
      'same:"a": 1,',
      'removed:"b": 2',
      'added:"b": 3,',
      'added:"c": 4',
      'same:}',
    ]);
  });

  it('shows the first version as it is', () => {
    expect(diffLines(null, 'a\nb').every((l) => l.kind === 'same')).toBe(true);
    expect(diffLines('a\nb', 'a\nb').every((l) => l.kind === 'same')).toBe(true);
  });
});
