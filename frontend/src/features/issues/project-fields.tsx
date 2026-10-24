import { ProjectStatusGlyph } from '@/components/custom-ui/issue-glyphs';
import { OptionSelect } from '@/components/custom-ui/option-select';
import type { ProjectStatus } from '@/schemas/issue';
import { teamLabel } from '@/schemas/workspace';

import { PROJECT_STATUS_OPTIONS } from './project-status';

const NONE = '__none__';

const STATUS_OPTIONS = PROJECT_STATUS_OPTIONS.map(({ value, label }) => ({
  value,
  label: (
    <span className="flex items-center gap-2">
      <ProjectStatusGlyph status={value} />
      {label}
    </span>
  ),
}));

/** Where a project stands, each status with its glyph. */
export function ProjectStatusSelect({
  value,
  onValueChange,
  className,
}: {
  value: ProjectStatus;
  onValueChange: (status: ProjectStatus) => void;
  className?: string;
}) {
  return (
    <OptionSelect
      value={value}
      onValueChange={onValueChange}
      options={STATUS_OPTIONS}
      aria-label="Status"
      className={className}
    />
  );
}

/** The team that owns a project: one of the given teams, or none. */
export function ProjectTeamSelect({
  teams,
  value,
  onValueChange,
  className,
}: {
  teams: { id: string; key: string; name: string }[];
  value: string | null;
  onValueChange: (teamId: string | null) => void;
  className?: string;
}) {
  return (
    <OptionSelect
      value={value ?? NONE}
      onValueChange={(id) => onValueChange(id === NONE ? null : id)}
      options={[
        { value: NONE, label: 'No team' },
        ...teams.map((team) => ({ value: team.id, label: teamLabel(team) })),
      ]}
      aria-label="Team"
      className={className}
    />
  );
}

/** Who leads a project: one of the workspace's members, or nobody. */
export function ProjectLeadSelect({
  members,
  value,
  onValueChange,
  className,
}: {
  members: { user_id: string; name: string }[];
  value: string | null;
  onValueChange: (userId: string | null) => void;
  className?: string;
}) {
  return (
    <OptionSelect
      value={value ?? NONE}
      onValueChange={(id) => onValueChange(id === NONE ? null : id)}
      options={[
        { value: NONE, label: 'No lead' },
        ...members.map((m) => ({ value: m.user_id, label: m.name })),
      ]}
      aria-label="Lead"
      className={className}
    />
  );
}
