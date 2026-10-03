import { cn } from '@/lib/utils';
import type { AgentStatus } from '@/schemas/agent';

const META: Record<AgentStatus, { label: string; color: string }> = {
  active: { label: 'Active', color: 'var(--status-succeeded)' },
  paused: { label: 'Paused', color: 'var(--status-idle)' },
  over_budget: { label: 'Over budget', color: 'var(--status-failed)' },
};

export function AgentStatusBadge({
  status,
  className,
}: {
  status: AgentStatus;
  className?: string;
}) {
  const meta = META[status];
  return (
    <span
      style={{ '--badge-color': meta.color } as React.CSSProperties}
      className={cn(
        'inline-flex h-5 items-center gap-1.5 rounded-full px-2 text-[11px] font-medium text-[var(--badge-color)]',
        'bg-[color-mix(in_oklch,var(--badge-color)_12%,transparent)]',
        className,
      )}
    >
      <span className="size-1.5 rounded-full bg-[var(--badge-color)]" />
      {meta.label}
    </span>
  );
}
