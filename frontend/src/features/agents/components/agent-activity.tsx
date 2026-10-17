import { useQuery } from '@tanstack/react-query';

import { Skeleton } from '@/components/ui/skeleton';
import { SPAN_KIND_META } from '@/features/runs/span-kinds';
import { formatRelative } from '@/lib/format';
import { cn } from '@/lib/utils';

import { agentActivityQuery } from '../api';

/** The agent's last few actions across runs, newest first. */
export function AgentActivity({ agentId, limit = 3 }: { agentId: string; limit?: number }) {
  const { data: spans, isPending } = useQuery(agentActivityQuery(agentId, limit));

  if (isPending) return <Skeleton className="h-12 rounded-md" />;
  if (!spans || spans.length === 0) {
    return <p className="text-xs text-muted-foreground">No activity yet</p>;
  }
  return (
    <ul className="space-y-1" aria-label="Recent activity">
      {spans.map((span) => {
        const meta = SPAN_KIND_META[span.kind];
        const Icon = meta.icon;
        const error = span.status === 'error';
        return (
          <li key={span.id} className="flex items-center gap-1.5 text-xs">
            <Icon
              className={cn('size-3 shrink-0 text-muted-foreground', error && 'text-destructive')}
              aria-label={meta.label}
            />
            <span className={cn('truncate font-mono', error && 'text-destructive')}>
              {span.name}
            </span>
            <span className="ml-auto shrink-0 text-muted-foreground">
              {formatRelative(span.started_at)}
            </span>
          </li>
        );
      })}
    </ul>
  );
}
