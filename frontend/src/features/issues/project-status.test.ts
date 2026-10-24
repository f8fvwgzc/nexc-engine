import { describe, expect, it } from 'vitest';

import type { ProjectStatus } from '@/schemas/issue';

import { PROJECT_LIST_ORDER, groupProjectsByStatus, projectProgress } from './project-status';

const project = (name: string, status: ProjectStatus) => ({ name, status });

const projects = [
  project('Docs migration', 'completed'),
  project('EU market entry', 'started'),
  project('Firewall hardening', 'planned'),
  project('Onboarding', 'started'),
];

describe('projects by status', () => {
  it("follows a project's life and leaves out statuses without projects", () => {
    const groups = groupProjectsByStatus(projects);
    expect(groups.map((g) => g.status)).toEqual(['planned', 'started', 'completed']);
    expect(groups.map((g) => g.label)).toEqual(['Planned', 'In progress', 'Completed']);
  });

  it('keeps the order the projects came in within a group', () => {
    const started = groupProjectsByStatus(projects).find((g) => g.status === 'started');
    expect(started?.projects.map((p) => p.name)).toEqual(['EU market entry', 'Onboarding']);
  });

  it('keeps every column of a board, empty or not', () => {
    const columns = groupProjectsByStatus(projects, { keepEmpty: true });
    expect(columns.map((c) => c.status)).toEqual([
      'planned',
      'started',
      'paused',
      'completed',
      'canceled',
    ]);
    expect(columns.find((c) => c.status === 'paused')?.projects).toEqual([]);
  });

  it('lists what is being worked on first', () => {
    const groups = groupProjectsByStatus(projects, { order: PROJECT_LIST_ORDER });
    expect(groups.map((g) => g.status)).toEqual(['started', 'planned', 'completed']);
  });

  it('has nothing to group without projects', () => {
    expect(groupProjectsByStatus([])).toEqual([]);
    expect(groupProjectsByStatus([], { keepEmpty: true })).toHaveLength(5);
  });
});

describe('project progress', () => {
  it('is the share of closed issues, rounded', () => {
    expect(projectProgress({ issue_count: 9, closed_count: 4 })).toBe(44);
    expect(projectProgress({ issue_count: 6, closed_count: 6 })).toBe(100);
  });

  it('is nothing for a project without issues', () => {
    expect(projectProgress({ issue_count: 0, closed_count: 0 })).toBe(0);
  });
});
