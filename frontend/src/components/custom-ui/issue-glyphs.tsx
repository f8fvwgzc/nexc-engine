import type { StateCategory } from '@/schemas/issue';
import { cn } from '@/lib/utils';

const SIZE = 14;
const R = 5.5;
const C = SIZE / 2;

/**
 * A workflow state as a small ring, in the state's own colour. The fill says how far along the
 * state is: dashed for backlog, empty for not started, half for in progress, full with a tick
 * when done, full with a cross when canceled.
 */
export function StateGlyph({
  category,
  color,
  className,
}: {
  category: StateCategory;
  color: string;
  className?: string;
}) {
  const closed = category === 'completed' || category === 'canceled';
  return (
    <svg
      width={SIZE}
      height={SIZE}
      viewBox={`0 0 ${SIZE} ${SIZE}`}
      aria-hidden
      className={cn('shrink-0', className)}
      style={{ color }}
    >
      <circle
        cx={C}
        cy={C}
        r={R}
        fill={closed ? 'currentColor' : 'none'}
        stroke="currentColor"
        strokeWidth={1.5}
        strokeDasharray={category === 'backlog' ? '1.6 2' : undefined}
      />
      {category === 'started' && (
        // Half of a disc inside the ring.
        <path d={`M${C} ${C - 3} A3 3 0 0 1 ${C} ${C + 3} Z`} fill="currentColor" />
      )}
      {category === 'completed' && (
        <path
          d="M4.4 7.2 6.2 9 9.7 5.3"
          fill="none"
          stroke="var(--background)"
          strokeWidth={1.5}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      )}
      {category === 'canceled' && (
        <path
          d="M5 5 9 9 M9 5 5 9"
          fill="none"
          stroke="var(--background)"
          strokeWidth={1.5}
          strokeLinecap="round"
        />
      )}
    </svg>
  );
}

const PRIORITY_NAME = ['No priority', 'Urgent', 'High', 'Medium', 'Low'] as const;

/** Priority as signal bars: three for high, two for medium, one for low; a filled mark for urgent. */
export function PriorityGlyph({ priority, className }: { priority: number; className?: string }) {
  const label = PRIORITY_NAME[priority] ?? PRIORITY_NAME[0];
  const base = cn('shrink-0', className);
  if (priority === 1) {
    return (
      <svg
        width={SIZE}
        height={SIZE}
        viewBox="0 0 14 14"
        role="img"
        aria-label={label}
        className={cn(base, 'text-destructive')}
      >
        <rect x="1" y="1" width="12" height="12" rx="3" fill="currentColor" />
        <path d="M7 3.6v4.2" stroke="var(--background)" strokeWidth={1.6} strokeLinecap="round" />
        <circle cx="7" cy="10.2" r="0.9" fill="var(--background)" />
      </svg>
    );
  }
  // 2 high -> 3 bars lit, 3 medium -> 2, 4 low -> 1, 0 none -> 0.
  const lit = priority >= 2 && priority <= 4 ? 5 - priority : 0;
  return (
    <svg
      width={SIZE}
      height={SIZE}
      viewBox="0 0 14 14"
      role="img"
      aria-label={label}
      className={cn(base, 'text-foreground')}
    >
      {[0, 1, 2].map((i) => (
        <rect
          key={i}
          x={2 + i * 4}
          y={9 - i * 3}
          width="2.4"
          height={3 + i * 3}
          rx="0.8"
          fill="currentColor"
          opacity={lit === 0 ? 0.25 : i < lit ? 0.9 : 0.25}
        />
      ))}
    </svg>
  );
}

/** A person as their initials in a small disc. */
export function PersonGlyph({ name, className }: { name: string | undefined; className?: string }) {
  const initials = (name ?? '')
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase())
    .join('');
  return (
    <span
      title={name ?? 'Unassigned'}
      aria-label={name ?? 'Unassigned'}
      className={cn(
        'flex size-5 shrink-0 items-center justify-center rounded-full text-[9px] font-medium',
        name ? 'bg-foreground/10 text-foreground' : 'border border-dashed text-muted-foreground',
        className,
      )}
    >
      {initials}
    </span>
  );
}
