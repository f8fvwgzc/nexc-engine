import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { SparklesIcon } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';

import { daySummaryQuery, writeDaySummary } from './api';

/**
 * The day in a few lines, written by the workspace's model when an admin asks. It is kept, so
 * opening the day again costs nothing; when more has happened since, it says so and can be
 * written again. `entries` is how much the day's timeline shows now.
 */
export function DaySummaryCard({
  workspaceId,
  day,
  entries,
}: {
  workspaceId: string;
  day: string;
  entries: number;
}) {
  const queryClient = useQueryClient();
  const query = daySummaryQuery(workspaceId, day);
  const { data: summary, isPending, error } = useQuery(query);
  const write = useMutation({
    mutationFn: () => writeDaySummary(workspaceId, day),
    onSuccess: (written) => queryClient.setQueryData(query.queryKey, written),
  });
  if (isPending) return <Skeleton className="h-16 w-full rounded-lg" />;
  const problem = write.error ?? error;
  return (
    <section aria-label="Summary of the day" className="space-y-2 rounded-lg border p-3">
      <div className="flex flex-wrap items-center gap-2">
        <SparklesIcon aria-hidden className="size-3.5 text-muted-foreground" />
        <h2 className="text-[13px] font-medium">Summary</h2>
        {summary && (
          <span className="text-xs text-muted-foreground">
            from {summary.event_count} {summary.event_count === 1 ? 'entry' : 'entries'} ·{' '}
            {summary.model} · {formatRelative(summary.created_at)}
            {summary.created_by ? ` · asked by ${summary.created_by}` : ''}
          </span>
        )}
        <Button
          variant={summary && !summary.stale ? 'ghost' : 'outline'}
          size="sm"
          className="ml-auto"
          disabled={write.isPending || entries === 0}
          onClick={() => write.mutate()}
        >
          {write.isPending
            ? 'Writing…'
            : !summary
              ? 'Summarise this day'
              : summary.stale
                ? 'Update summary'
                : 'Write again'}
        </Button>
      </div>
      {summary ? (
        <div className="space-y-2 text-[13px]">
          <p className="font-medium">{summary.headline}</p>
          {summary.highlights.length > 0 && (
            <ul className="list-disc space-y-0.5 pl-5">
              {summary.highlights.map((point) => (
                <li key={point}>{point}</li>
              ))}
            </ul>
          )}
          {summary.attention.length > 0 && (
            <div>
              <p className="text-xs font-medium text-muted-foreground">Worth a look</p>
              <ul className="list-disc space-y-0.5 pl-5">
                {summary.attention.map((point) => (
                  <li key={point}>{point}</li>
                ))}
              </ul>
            </div>
          )}
          {summary.stale && (
            <p role="status" className="text-xs text-muted-foreground">
              More has happened on this day since the summary was written.
            </p>
          )}
        </div>
      ) : (
        <p className="text-xs text-muted-foreground">
          {entries === 0
            ? 'Nothing happened on this day, so there is nothing to summarise.'
            : 'Have the workspace’s AI tell this day in a few lines. It uses your AI account or the workspace’s, counts toward the token budget, and is kept so that reading it again costs nothing.'}
        </p>
      )}
      {problem ? (
        <p role="alert" className="text-xs text-destructive">
          {errorMessage(problem)}
        </p>
      ) : null}
    </section>
  );
}
