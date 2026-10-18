import { AlertTriangleIcon, RotateCcwIcon } from 'lucide-react';
import type { ReactNode } from 'react';

import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/lib/api/errors';
import { cn } from '@/lib/utils';

interface ErrorStateProps {
  /** What could not be had, e.g. "Couldn't load the inbox". */
  title?: string;
  error: unknown;
  /** Asks again; shown as "Try again". */
  onRetry?: () => void;
  /** Another way out, beside "Try again". */
  action?: ReactNode;
  /** A smaller frame, for a panel or a section of a page. */
  compact?: boolean;
  className?: string;
}

/**
 * What a page or a panel shows when its data could not be loaded: the same frame as an empty
 * state, so the two never look alike by accident, with the reason and a way to ask again.
 */
export function ErrorState({
  title = 'Couldn’t load this',
  error,
  onRetry,
  action,
  compact = false,
  className,
}: ErrorStateProps) {
  return (
    <Empty
      role="alert"
      data-state="error"
      className={cn(
        'border border-dashed border-destructive/30 bg-destructive/[0.03]',
        compact ? 'gap-3 p-4 py-6' : 'py-12',
        className,
      )}
    >
      <EmptyHeader>
        <EmptyMedia
          variant="icon"
          className="size-11 rounded-xl bg-destructive/10 text-destructive"
        >
          <AlertTriangleIcon className="size-5" aria-hidden />
        </EmptyMedia>
        <EmptyTitle>{title}</EmptyTitle>
        <EmptyDescription>{errorMessage(error)}</EmptyDescription>
      </EmptyHeader>
      {(onRetry || action) && (
        <EmptyContent>
          <div className="flex flex-wrap justify-center gap-2">
            {onRetry && (
              <Button variant="outline" onClick={onRetry}>
                <RotateCcwIcon />
                Try again
              </Button>
            )}
            {action}
          </div>
        </EmptyContent>
      )}
    </Empty>
  );
}
