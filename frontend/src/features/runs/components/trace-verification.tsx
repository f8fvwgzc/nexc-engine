import { useQuery } from '@tanstack/react-query';
import { RefreshCwIcon, ShieldAlertIcon, ShieldCheckIcon, ShieldIcon } from 'lucide-react';

import { IconButton } from '@/components/custom-ui/icon-button';
import { Skeleton } from '@/components/ui/skeleton';
import { errorMessage } from '@/lib/api/errors';

import { traceVerifyQuery } from '../api';

/**
 * Whether the run's hash chain still holds, as the server recomputes it. A live run is not sealed
 * yet; it is checked once it finishes.
 */
export function TraceVerification({ runId, active }: { runId: string; active: boolean }) {
  const verify = useQuery({ ...traceVerifyQuery(runId), enabled: !active });

  if (active) {
    return (
      <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <ShieldIcon className="size-3.5" aria-hidden />
        Sealed when the run finishes
      </p>
    );
  }
  if (verify.isPending) return <Skeleton className="h-6 w-48 rounded-full" />;
  const again = (
    <IconButton
      label="Verify the trace again"
      disabled={verify.isFetching}
      onClick={() => void verify.refetch()}
    >
      <RefreshCwIcon className={verify.isFetching ? 'motion-safe:animate-spin' : undefined} />
    </IconButton>
  );
  if (verify.isError) {
    return (
      <p className="flex items-center gap-1.5 text-xs text-destructive">
        {errorMessage(verify.error)}
        {again}
      </p>
    );
  }
  const result = verify.data;
  if (result.ok) {
    return (
      <p className="flex items-center gap-1.5 text-xs text-[var(--status-succeeded)]" role="status">
        <ShieldCheckIcon className="size-3.5" aria-hidden />
        <span>
          Chain intact · {result.spans} spans
          {result.hash && (
            <span className="font-mono text-muted-foreground" title={result.hash}>
              {' '}
              · {result.hash.slice(0, 12)}
            </span>
          )}
        </span>
        {again}
      </p>
    );
  }
  return (
    <p className="flex items-center gap-1.5 text-xs text-destructive" role="alert">
      <ShieldAlertIcon className="size-3.5 shrink-0" aria-hidden />
      <span>
        {result.broken
          ? `Chain broken at span #${result.broken.seq} (${result.broken.span_id.slice(0, 8)}): ${result.broken.reason}`
          : 'Chain broken: the sealed hash has no spans behind it'}
      </span>
      {again}
    </p>
  );
}
