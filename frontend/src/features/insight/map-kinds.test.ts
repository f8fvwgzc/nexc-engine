import { describe, expect, it } from 'vitest';

import { countOf, followable, FOLLOWABLE, nameOf, pageOf } from './map-kinds';

const item = (kind: string) => ({
  kind,
  id: '01a10c67-ce89-7272-8865-1447210e01b4',
  title: 't',
  subtitle: '',
});

describe('map kinds', () => {
  it('counts in words, one and many', () => {
    expect(countOf('team', 1)).toBe('1 team');
    expect(countOf('memory', 2341)).toBe('2,341 memories');
    expect(countOf('widget', 2)).toBe('2 widgets');
    expect(nameOf('member')).toBe('member');
    expect(nameOf('widget')).toBe('widget');
  });

  it('follows the kinds the server can open, and only names the others', () => {
    expect(followable('issue')).toBe('issue');
    expect(followable('label')).toBeNull();
    expect(FOLLOWABLE.map((k) => k.label)).toContain('Members');
  });

  it('links a thing to its page when it has one', () => {
    expect(pageOf(item('issue'))).toBe('/app/issues?issue=01a10c67-ce89-7272-8865-1447210e01b4');
    expect(pageOf(item('graph'))).toBe('/app/graphs/01a10c67-ce89-7272-8865-1447210e01b4');
    expect(pageOf(item('label'))).toBeNull();
  });
});
