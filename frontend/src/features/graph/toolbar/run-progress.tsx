import { ArrowUpRightIcon, CoinsIcon } from 'lucide-react';
import { Link } from 'react-router-dom';
import { useShallow } from 'zustand/react/shallow';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { StatusBadge } from '@/components/custom-ui/status-badge';
import { Progress } from '@/components/ui/progress';
import { formatCost, formatTokens } from '@/lib/format';
import { selectRunProgress, useGraphStore } from '@/stores/graph-store';

/** Live run summary pinned to the bottom of the canvas. */
export function RunProgress() {
  const run = useGraphStore((s) => s.run);
  const progress = useGraphStore(useShallow(selectRunProgress));
  if (!run || !progress) return null;
  const percent = progress.total ? Math.round((progress.done / progress.total) * 100) : 0;

  return (
    <GlassCard className="pointer-events-auto flex w-full max-w-2xl flex-wrap items-center gap-x-4 gap-y-2 px-3 py-2 text-xs motion-safe:animate-fade-up">
      <StatusBadge status={run.status} />
      <div className="flex min-w-32 flex-1 items-center gap-2">
        <Progress
          value={percent}
          aria-label="Run progress"
          className="h-1.5 [&_[data-slot=progress-indicator]]:bg-gradient-to-r [&_[data-slot=progress-indicator]]:from-brand [&_[data-slot=progress-indicator]]:to-brand-2"
        />
        <span className="shrink-0 text-muted-foreground tabular-nums">
          {progress.done}/{progress.total}
        </span>
      </div>
      <span className="text-muted-foreground tabular-nums" title="Tokens in / out">
        {formatTokens(progress.tokensIn)} in · {formatTokens(progress.tokensOut)} out
      </span>
      <span className="flex items-center gap-1 text-muted-foreground tabular-nums" title="Cost">
        <CoinsIcon className="size-3.5" aria-hidden />
        {formatCost(run.cost_usd)}
      </span>
      <Link
        to={`/app/runs/${run.id}`}
        className="ml-auto flex items-center gap-0.5 font-medium text-foreground underline-offset-4 hover:underline"
      >
        Details
        <ArrowUpRightIcon className="size-3.5" aria-hidden />
      </Link>
    </GlassCard>
  );
}
