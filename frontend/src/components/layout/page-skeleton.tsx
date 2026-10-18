import { PageContainer, type PageWidth } from '@/components/custom-ui/page-container';
import { Skeleton } from '@/components/ui/skeleton';

/**
 * A whole page while it loads: a header and a card grid in the page's own frame, so nothing
 * moves when the page arrives. `width` is the width of the page that will replace it.
 */
export function PageSkeleton({ width = 'default' }: { width?: PageWidth }) {
  return (
    <PageContainer width={width} role="status" aria-busy aria-label="Loading">
      <div className="space-y-2 border-b pb-4">
        <Skeleton className="h-7 w-48" />
        <Skeleton className="h-4 w-80 max-w-full" />
      </div>
      <Cards />
    </PageContainer>
  );
}

/** The body of a page while it loads, under a header that is already there. */
export function ContentSkeleton() {
  return (
    <div role="status" aria-busy aria-label="Loading">
      <Cards />
    </div>
  );
}

/** A table while it loads: its header row and a few rows. */
export function TableSkeleton({ rows = 6 }: { rows?: number }) {
  return (
    <div className="space-y-2" role="status" aria-busy aria-label="Loading">
      <Skeleton className="h-9 w-full" />
      {Array.from({ length: rows }, (_, i) => (
        <Skeleton key={i} className="h-12 w-full" />
      ))}
    </div>
  );
}

function Cards() {
  return (
    <div className="space-y-6">
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {Array.from({ length: 4 }, (_, i) => (
          <Skeleton key={i} className="h-24 rounded-xl" />
        ))}
      </div>
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {Array.from({ length: 6 }, (_, i) => (
          <Skeleton key={i} className="h-36 rounded-xl" />
        ))}
      </div>
    </div>
  );
}
