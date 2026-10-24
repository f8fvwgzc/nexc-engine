import { describe, expect, it } from 'vitest';

import { githubLabel, githubRef } from './github';

describe('GitHub links', () => {
  it('read as owner/repo#number, shorter where space is tight', () => {
    const url = 'https://github.com/acme/platform/issues/212';
    expect(githubLabel(url)).toBe('acme/platform#212');
    expect(githubLabel(url, 'repo')).toBe('platform#212');
    expect(githubLabel(url, 'number')).toBe('#212');
  });

  it('tell an issue from a pull request', () => {
    expect(githubRef('https://github.com/acme/agent-runtime/pull/87')).toEqual({
      owner: 'acme',
      repo: 'agent-runtime',
      number: 87,
      kind: 'pull',
    });
    expect(githubRef('https://github.com/acme/platform/issues/3')?.kind).toBe('issue');
  });

  it('accept the forms the server normalises', () => {
    expect(githubLabel('http://www.github.com/acme/my.repo/issues/9/')).toBe('acme/my.repo#9');
    expect(githubLabel('https://github.com/acme/platform/pull/5#discussion_r1')).toBe(
      'acme/platform#5',
    );
  });

  it('name anything else plainly', () => {
    expect(githubRef('https://gitlab.com/acme/platform/issues/1')).toBeNull();
    expect(githubRef('https://github.com/acme/platform')).toBeNull();
    expect(githubLabel('not a url')).toBe('GitHub');
  });
});
