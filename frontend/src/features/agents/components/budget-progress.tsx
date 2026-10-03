import { Progress } from '@/components/ui/progress';
import { formatTokens } from '@/lib/format';
import { cn } from '@/lib/utils';

/** Spent vs budget tokens. A budget of 0 means "no limit". */
export function BudgetProgress({
  spent,
  budget,
  className,
}: {
  spent: number;
  budget: number;
  className?: string;
}) {
  if (budget <= 0) {
    return (
      <span className={cn('text-xs text-muted-foreground tabular-nums', className)}>
        {formatTokens(spent)} used · no limit
      </span>
    );
  }
  const percent = Math.min(100, Math.round((spent / budget) * 100));
  const tone =
    percent >= 90
      ? '[&_[data-slot=progress-indicator]]:bg-status-failed'
      : percent >= 70
        ? '[&_[data-slot=progress-indicator]]:bg-status-queued'
        : '[&_[data-slot=progress-indicator]]:bg-status-succeeded';
  return (
    <div className={cn('min-w-32 space-y-1', className)}>
      <Progress
        value={percent}
        className={cn('h-1.5', tone)}
        aria-label={`Budget ${percent}% used`}
      />
      <p className="text-[11px] text-muted-foreground tabular-nums">
        {formatTokens(spent)} / {formatTokens(budget)} tokens · {percent}%
      </p>
    </div>
  );
}
