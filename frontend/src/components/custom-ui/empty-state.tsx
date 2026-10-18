import type { LucideIcon } from 'lucide-react';
import type { ReactNode } from 'react';

import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty';
import { cn } from '@/lib/utils';

interface EmptyStateProps {
  icon: LucideIcon;
  title: string;
  /** One sentence: what will be here, and how it gets here. */
  description?: ReactNode;
  /** The next step: a button or a link. */
  action?: ReactNode;
  /** The title's heading level, when the empty state is the page (a 404 is `1`). */
  headingLevel?: 1 | 2 | 3;
  /** A smaller frame, for a panel or a section of a page. */
  compact?: boolean;
  className?: string;
}

export function EmptyState({
  icon: Icon,
  title,
  description,
  action,
  headingLevel,
  compact = false,
  className,
}: EmptyStateProps) {
  return (
    <Empty
      className={cn(
        'border border-dashed bg-gradient-to-b from-muted/30 to-transparent motion-safe:animate-fade-in',
        compact ? 'gap-3 p-4 py-6' : 'py-12',
        className,
      )}
    >
      <EmptyHeader>
        <EmptyMedia variant="icon" className="size-11 rounded-xl bg-brand/10 text-brand">
          <Icon className="size-5" aria-hidden />
        </EmptyMedia>
        <EmptyTitle {...(headingLevel ? { role: 'heading', 'aria-level': headingLevel } : {})}>
          {title}
        </EmptyTitle>
        {description && <EmptyDescription>{description}</EmptyDescription>}
      </EmptyHeader>
      {action && <EmptyContent>{action}</EmptyContent>}
    </Empty>
  );
}
