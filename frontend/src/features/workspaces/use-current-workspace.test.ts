import { describe, expect, it } from 'vitest';

import type { Workspace } from '@/schemas/workspace';

import { defaultWorkspace } from './use-current-workspace';

const workspace = (id: string, member_count: number) => ({ id, member_count }) as Workspace;

describe('defaultWorkspace', () => {
  it('opens the shared workspace rather than the older personal one', () => {
    const all = [workspace('personal', 1), workspace('acme', 5), workspace('side', 2)];
    expect(defaultWorkspace(all)?.id).toBe('acme');
  });

  it('keeps the first on a tie, and has nothing to open without workspaces', () => {
    expect(defaultWorkspace([workspace('a', 1), workspace('b', 1)])?.id).toBe('a');
    expect(defaultWorkspace([])).toBeUndefined();
  });
});
