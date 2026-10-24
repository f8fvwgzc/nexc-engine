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

describe('qk.issues.lists', () => {
  it('reaches every issue list of the workspace, and nothing else of it', async () => {
    const client = new QueryClient();
    const keys = [
      qk.issues.list('w', {}),
      qk.issues.list('w', { team_id: 't' }),
      qk.issues.list('other', {}),
      qk.issues.labels('w'),
      qk.issues.one('i'),
    ];
    for (const key of keys) client.setQueryData(key, []);
    await client.invalidateQueries({ queryKey: qk.issues.lists('w') });
    const stale = keys.map((key) => client.getQueryState(key)?.isInvalidated);
    expect(stale).toEqual([true, true, false, false, false]);
  });
});
