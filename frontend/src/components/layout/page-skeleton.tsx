import type { CSSProperties } from 'react';

import { PageContainer, type PageWidth } from '@/components/custom-ui/page-container';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';

/*
 * Placeholders shaped like the pages they stand in for, so nothing moves when the page arrives.
 * Each exported one is a single busy region (`role="status"`, `aria-busy`); the pieces inside are
 * plain shapes. A page placeholder has the page's own frame and width; a section placeholder goes
 * under a header that is already there.
 */

/** The page's header: its title and a line of description over the header's rule. */
function HeaderShape({ action = true }: { action?: boolean }) {
  return (
    <div className="flex items-end justify-between gap-6 border-b pb-4">
      <div className="min-w-0 flex-1 space-y-2">
        <Skeleton className="h-6 w-48" />
        <Skeleton className="h-4 w-80 max-w-full" />
      </div>
      {action && <Skeleton className="hidden h-8 w-28 shrink-0 sm:block" />}
    </div>
  );
}

/** A list page's filters: a select or two, a search field and the view switch. */
function FilterShape() {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Skeleton className="h-8 w-52" />
      <Skeleton className="h-8 w-40" />
      <Skeleton className="hidden h-8 w-56 sm:block" />
      <Skeleton className="ml-auto h-8 w-16" />
    </div>
  );
}

/** A title of varying length, so a column of rows does not look like a barcode. */
const varied = (i: number, from = 40, span = 45): CSSProperties => ({
  maxWidth: `${from + ((i * 13) % span)}%`,
});

/** Rows of a list: a glyph, a title and something at the end, in a bordered box. */
function RowsShape({ rows, className }: { rows: number; className?: string }) {
  return (
    <div className={cn('overflow-hidden rounded-lg border', className)}>
      <Skeleton className="h-8 w-full rounded-none" />
      <ul className="divide-y">
        {Array.from({ length: rows }, (_, i) => (
          <li key={i} className="flex h-10 items-center gap-3 px-3">
            <Skeleton className="size-3.5 shrink-0 rounded" />
            <Skeleton className="h-3 flex-1" style={varied(i)} />
            <Skeleton className="ml-auto h-3 w-12 shrink-0" />
          </li>
        ))}
      </ul>
    </div>
  );
}

/** Stat tiles in a row: two to a line on a phone, all of them on a wide screen. */
function StatsShape({ count }: { count: number }) {
  return (
    <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
      {Array.from({ length: count }, (_, i) => (
        <Skeleton key={i} className="h-22 rounded-xl" />
      ))}
    </div>
  );
}

/** A settings card: its title, a sentence, and a few fields or rows. */
function CardShape({ rows = 3 }: { rows?: number }) {
  return (
    <div className="space-y-4 rounded-xl border p-5">
      <div className="space-y-2">
        <Skeleton className="h-5 w-40" />
        <Skeleton className="h-3.5 w-72 max-w-full" />
      </div>
      <div className="space-y-2.5">
        {Array.from({ length: rows }, (_, i) => (
          <Skeleton key={i} className="h-9 w-full" style={varied(i, 70, 30)} />
        ))}
      </div>
    </div>
  );
}

function CardsShape() {
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

/**
 * A whole page while it loads: a header and a card grid in the page's own frame, so nothing
 * moves when the page arrives. `width` is the width of the page that will replace it.
 */
export function PageSkeleton({ width = 'default' }: { width?: PageWidth }) {
  return (
    <PageContainer width={width} role="status" aria-busy aria-label="Loading">
      <HeaderShape action={false} />
      <CardsShape />
    </PageContainer>
  );
}

/** A list page while it loads: its header, its filters and its first rows. */
export function ListPageSkeleton({
  width = 'default',
  filters = true,
  rows = 8,
}: {
  width?: PageWidth;
  /** Whether the page has a filter bar under its header. */
  filters?: boolean;
  rows?: number;
}) {
  return (
    <PageContainer width={width} role="status" aria-busy aria-label="Loading">
      <HeaderShape />
      {filters && <FilterShape />}
      <RowsShape rows={rows} />
    </PageContainer>
  );
}

/** A list's rows while they load, under a header (and filters) already on screen. */
export function ListSkeleton({
  rows = 6,
  label = 'Loading',
  className,
}: {
  rows?: number;
  /** What is loading, e.g. "Loading the inbox". */
  label?: string;
  className?: string;
}) {
  return (
    <div role="status" aria-busy aria-label={label} className={className}>
      <RowsShape rows={rows} />
    </div>
  );
}

/**
 * One thing on its own page while it loads (a project, a run): its title and description, its
 * properties and, under them, its content. `stats` puts a row of stat tiles first; `aside` puts
 * the properties in a column beside the content instead.
 */
export function DetailSkeleton({
  width = 'default',
  stats = 0,
  aside = false,
}: {
  width?: PageWidth;
  stats?: number;
  aside?: boolean;
}) {
  const main = (
    <>
      <div className="space-y-2.5">
        <Skeleton className="h-7 w-72 max-w-full" />
        <Skeleton className="h-4 w-full max-w-2xl" />
        <Skeleton className="h-4 w-4/6 max-w-xl" />
      </div>
      {!aside && (
        <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
          {Array.from({ length: 5 }, (_, i) => (
            <Skeleton key={i} className="h-7 w-32" />
          ))}
        </div>
      )}
      {stats > 0 && <StatsShape count={stats} />}
      <RowsShape rows={5} />
    </>
  );
  return (
    <PageContainer width={width} role="status" aria-busy aria-label="Loading">
      {aside ? (
        <div className="flex flex-col gap-6 md:flex-row">
          <div className="min-w-0 flex-1 space-y-6">{main}</div>
          <div className="space-y-2 md:w-72">
            {Array.from({ length: 7 }, (_, i) => (
              <Skeleton key={i} className="h-8 w-full" />
            ))}
          </div>
        </div>
      ) : (
        main
      )}
    </PageContainer>
  );
}

/** A board while it loads: columns of cards, under a header and filters already on screen. */
export function BoardSkeleton({ columns = 4 }: { columns?: number }) {
  return (
    <div
      role="status"
      aria-busy
      aria-label="Loading"
      className="flex min-h-0 flex-1 gap-3 overflow-hidden pb-2"
    >
      {Array.from({ length: columns }, (_, column) => (
        <div key={column} className="w-72 shrink-0 space-y-2 rounded-xl border bg-muted/30 p-2">
          <Skeleton className="h-5 w-28" />
          {Array.from({ length: 3 - (column % 2) }, (_, i) => (
            <Skeleton key={i} className="h-20 w-full rounded-lg" />
          ))}
        </div>
      ))}
    </div>
  );
}

/** A settings page while it loads: its header and its cards, as narrow as the page. */
export function SettingsSkeleton({ cards = 2 }: { cards?: number }) {
  return (
    <PageContainer width="narrow" role="status" aria-busy aria-label="Loading">
      <HeaderShape action={false} />
      {Array.from({ length: cards }, (_, i) => (
        <CardShape key={i} rows={i === 0 ? 3 : 2} />
      ))}
    </PageContainer>
  );
}

/** The cards of a settings page while they load, under its header. */
export function SettingsCardsSkeleton({
  cards = 1,
  rows = 3,
  label = 'Loading',
}: {
  cards?: number;
  rows?: number;
  label?: string;
}) {
  return (
    <div role="status" aria-busy aria-label={label} className="space-y-6">
      {Array.from({ length: cards }, (_, i) => (
        <CardShape key={i} rows={rows} />
      ))}
    </div>
  );
}

/** The body of a page while it loads, under a header that is already there. */
export function ContentSkeleton() {
  return (
    <div role="status" aria-busy aria-label="Loading">
      <CardsShape />
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
