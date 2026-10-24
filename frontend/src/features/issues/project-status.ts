import type { ProjectStatus } from '@/schemas/issue';

export const PROJECT_STATUS_OPTIONS: { value: ProjectStatus; label: string }[] = [
  { value: 'planned', label: 'Planned' },
  { value: 'started', label: 'In progress' },
  { value: 'paused', label: 'Paused' },
  { value: 'completed', label: 'Completed' },
  { value: 'canceled', label: 'Canceled' },
];

/** Order of a list's groups: what is being worked on first, as the issue list does. */
export const PROJECT_LIST_ORDER: ProjectStatus[] = [
  'started',
  'planned',
  'paused',
  'completed',
  'canceled',
];

export const projectStatusLabel = (status: ProjectStatus) =>
  PROJECT_STATUS_OPTIONS.find((o) => o.value === status)?.label ?? status;

/** Share of a project's issues that are closed, 0-100. */
export function projectProgress(project: { issue_count: number; closed_count: number }): number {
  return project.issue_count > 0
    ? Math.round((project.closed_count / project.issue_count) * 100)
    : 0;
}

export interface ProjectGroup<P> {
  status: ProjectStatus;
  label: string;
  projects: P[];
}

/**
 * Projects by status, each group in the order the projects came. Groups follow `order` (a
 * project's life, planned to canceled, by default); statuses without projects are left out
 * unless `keepEmpty`, as a board keeps its columns.
 */
export function groupProjectsByStatus<P extends { status: ProjectStatus }>(
  projects: readonly P[],
  {
    order = PROJECT_STATUS_OPTIONS.map((o) => o.value),
    keepEmpty = false,
  }: { order?: readonly ProjectStatus[]; keepEmpty?: boolean } = {},
): ProjectGroup<P>[] {
  return order
    .map((status) => ({
      status,
      label: projectStatusLabel(status),
      projects: projects.filter((p) => p.status === status),
    }))
    .filter((group) => keepEmpty || group.projects.length > 0);
}
