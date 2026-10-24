import { QueryClient } from '@tanstack/react-query';
import { describe, expect, it } from 'vitest';

import { qk } from './query-keys';

describe('qk.issues.projects', () => {
  it('keeps every team filter under the workspace’s project lists', () => {
    const lists = qk.issues.projectLists('w');
    expect(qk.issues.projects('w').slice(0, lists.length)).toEqual(lists);
    expect(qk.issues.projects('w', 't').slice(0, lists.length)).toEqual(lists);
  });

  it('tells one team’s list from everyone’s', () => {
    expect(qk.issues.projects('w', 't')).not.toEqual(qk.issues.projects('w'));
  });

  it('marks every project list of the workspace stale, and no other workspace’s', async () => {
    const client = new QueryClient();
    for (const key of [
      qk.issues.projects('w'),
      qk.issues.projects('w', 't'),
      qk.issues.projects('other'),
    ]) {
      client.setQueryData(key, []);
    }
    await client.invalidateQueries({ queryKey: qk.issues.projectLists('w') });
    const stale = (key: readonly unknown[]) => client.getQueryState(key)?.isInvalidated;
    expect(stale(qk.issues.projects('w'))).toBe(true);
    expect(stale(qk.issues.projects('w', 't'))).toBe(true);
    expect(stale(qk.issues.projects('other'))).toBe(false);
  });
});
