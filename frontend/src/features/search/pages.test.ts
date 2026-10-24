import { describe, expect, it } from 'vitest';

import { pageOf } from './pages';

const id = '01a10c67-ce89-7272-8865-1447210e01b4';

describe('search hits', () => {
  it('link to the page of what they found', () => {
    expect(pageOf({ kind: 'issue', id })).toBe(`/app/issues/${id}`);
    expect(pageOf({ kind: 'graph', id })).toBe(`/app/graphs/${id}`);
    expect(pageOf({ kind: 'team', id })).toBe(`/app/issues?team=${id}`);
    expect(pageOf({ kind: 'member', id })).toBe('/app/settings/members');
  });
});
