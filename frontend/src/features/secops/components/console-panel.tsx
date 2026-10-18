import type { LucideIcon } from 'lucide-react';
import type { ReactNode } from 'react';

import { cn } from '@/lib/utils';

interface PanelProps {
  /** Id of the heading, for `aria-labelledby`. */
  id: string;
  title: string;
  icon: LucideIcon;
  /** A number shown after the title (open approvals, lines in the feed). */
  count?: number;
  /** One line under the title. */
  hint?: ReactNode;
  actions?: ReactNode;
  className?: string;
  bodyClassName?: string;
  children: ReactNode;
}

/** One panel of the console: a titled frame with its own actions and a scrolling body. */
export function Panel({
  id,
  title,
  icon: Icon,
  count,
  hint,
  actions,
  className,
  bodyClassName,
  children,
}: PanelProps) {
  return (
    <section
      aria-labelledby={id}
      className={cn('flex min-h-0 flex-col rounded-xl border bg-card', className)}
    >
      <header className="flex flex-wrap items-center gap-2 border-b px-3 py-2">
        <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden />
        <h2 id={id} className="text-sm font-medium">
          {title}
          {count !== undefined && (
            <span className="ml-1.5 text-xs font-normal text-muted-foreground tabular-nums">
              {count}
            </span>
          )}
        </h2>
        {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
        {actions && <div className="ml-auto flex flex-wrap items-center gap-1.5">{actions}</div>}
      </header>
      <div className={cn('min-h-0 flex-1 p-3', bodyClassName)}>{children}</div>
    </section>
  );
}

/** A quiet line for an empty panel. */
export function PanelEmpty({ children }: { children: ReactNode }) {
  return <p className="px-1 py-6 text-center text-xs text-muted-foreground">{children}</p>;
}
