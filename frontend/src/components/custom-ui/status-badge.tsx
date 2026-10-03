import { cn } from '@/lib/utils';

import { STATUS_META, type DisplayStatus } from './status-meta';

interface StatusBadgeProps {
  status: DisplayStatus;
  className?: string;
  /** Icon-only dot (used in dense tables / canvas legends). */
  compact?: boolean;
}

/** Colored pill per node/run status; `running` pulses (disabled under reduced motion). */
export function StatusBadge({ status, className, compact = false }: StatusBadgeProps) {
  const meta = STATUS_META[status];
  const Icon = meta.icon;
  const running = status === 'running';
  return (
    <span
      role="status"
      aria-label={meta.label}
      style={{ '--badge-color': meta.color } as React.CSSProperties}
      className={cn(
        'inline-flex h-6 shrink-0 items-center gap-1.5 rounded-full border px-2 text-xs font-medium whitespace-nowrap',
        'border-[color-mix(in_oklch,var(--badge-color)_35%,transparent)] bg-[color-mix(in_oklch,var(--badge-color)_12%,transparent)] text-[var(--badge-color)]',
        running && 'motion-safe:animate-pulse-ring',
        compact && 'size-6 justify-center px-0',
        className,
      )}
    >
      <Icon className={cn('size-3.5', running && 'motion-safe:animate-spin')} aria-hidden />
      {!compact && meta.label}
    </span>
  );
}
