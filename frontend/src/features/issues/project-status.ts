import type { ProjectStatus } from '@/schemas/issue';

export const PROJECT_STATUS_OPTIONS: { value: ProjectStatus; label: string }[] = [
  { value: 'planned', label: 'Planned' },
  { value: 'started', label: 'In progress' },
  { value: 'paused', label: 'Paused' },
  { value: 'completed', label: 'Completed' },
  { value: 'canceled', label: 'Canceled' },
];

export const projectStatusLabel = (status: ProjectStatus) =>
  PROJECT_STATUS_OPTIONS.find((o) => o.value === status)?.label ?? status;

/** Share of a project's issues that are closed, 0-100. */
export function projectProgress(project: { issue_count: number; closed_count: number }): number {
  return project.issue_count > 0
    ? Math.round((project.closed_count / project.issue_count) * 100)
    : 0;
}
