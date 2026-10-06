import { useQuery } from '@tanstack/react-query';
import { ArrowUpRightIcon, ChevronRightIcon } from 'lucide-react';
import { useState } from 'react';
import { Link } from 'react-router-dom';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { errorMessage } from '@/lib/api/errors';
import type { MapItem, MapKind, MapTie } from '@/schemas/insight';

import { mapItemsQuery, mapNeighbourhoodQuery } from './api';
import { countOf, followable, FOLLOWABLE, nameOf, pageOf } from './map-kinds';

function ItemRow({ item, onFollow }: { item: MapItem; onFollow?: () => void }) {
  const body = (
    <>
      <span className="min-w-0 flex-1 truncate font-medium">{item.title}</span>
      {item.subtitle && (
        <span className="max-w-[45%] shrink-0 truncate text-xs text-muted-foreground">
          {item.subtitle}
        </span>
      )}
      {onFollow && <ChevronRightIcon aria-hidden className="size-3.5 text-muted-foreground" />}
    </>
  );
  const className = 'flex min-h-8 w-full items-center gap-2 px-3 py-1.5 text-left text-[13px]';
  return onFollow ? (
    <button
      type="button"
      className={`${className} hover:bg-muted/50 focus-visible:bg-muted/50 focus-visible:outline-none`}
      onClick={onFollow}
    >
      {body}
    </button>
  ) : (
    <div className={className}>{body}</div>
  );
}

/** The things of a kind, to pick the one whose ties to follow. */
function Picker({
  workspaceId,
  kind,
  onPick,
}: {
  workspaceId: string;
  kind: MapKind;
  onPick: (item: MapItem) => void;
}) {
  const [q, setQ] = useState('');
  const term = useDebouncedValue(q.trim(), 250);
  const { data: items, isPending, error } = useQuery(mapItemsQuery(workspaceId, kind, term));
  return (
    <div className="space-y-2">
      <Input
        type="search"
        value={q}
        aria-label="Find one"
        placeholder="Find one by name…"
        className="h-8 max-w-xs text-[13px]"
        onChange={(e) => setQ(e.target.value)}
      />
      {error ? (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(error)}
        </p>
      ) : isPending ? (
        <Skeleton className="h-40 w-full rounded-lg" />
      ) : items.length === 0 ? (
        <p className="rounded-lg border px-3 py-6 text-center text-[13px] text-muted-foreground">
          {term ? 'Nothing matches.' : 'The workspace has none yet.'}
        </p>
      ) : (
        <ul className="divide-y overflow-hidden rounded-lg border">
          {items.map((item) => (
            <li key={item.id}>
              <ItemRow item={item} onFollow={() => onPick(item)} />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Tie({ tie, onFollow }: { tie: MapTie; onFollow: (item: MapItem) => void }) {
  const more = tie.count - tie.items.length;
  return (
    <li className="space-y-1">
      <p className="text-xs text-muted-foreground">
        {tie.label}{' '}
        <span className="font-medium text-foreground">{countOf(tie.kind, tie.count)}</span>
      </p>
      {tie.items.length > 0 && (
        <ul className="divide-y overflow-hidden rounded-lg border">
          {tie.items.map((item) => (
            <li key={item.id}>
              <ItemRow
                item={item}
                onFollow={followable(item.kind) ? () => onFollow(item) : undefined}
              />
            </li>
          ))}
          {more > 0 && (
            <li className="px-3 py-1.5 text-xs text-muted-foreground">and {more} more</li>
          )}
        </ul>
      )}
    </li>
  );
}

/** One thing and everything it is tied to; a tied thing can be followed in turn. */
function Neighbourhood({
  workspaceId,
  item,
  kind,
  onFollow,
}: {
  workspaceId: string;
  item: MapItem;
  kind: MapKind;
  onFollow: (item: MapItem) => void;
}) {
  const { data, isPending, error } = useQuery(mapNeighbourhoodQuery(workspaceId, kind, item.id));
  const page = pageOf(item);
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <h3 className="min-w-0 truncate text-sm font-semibold">{data?.item.title ?? item.title}</h3>
        <span className="text-xs text-muted-foreground">
          {nameOf(item.kind)}
          {(data?.item.subtitle ?? item.subtitle) && ` · ${data?.item.subtitle ?? item.subtitle}`}
        </span>
        {page && (
          <Link
            to={page}
            className="ml-auto inline-flex items-center gap-1 text-xs text-muted-foreground underline-offset-2 hover:underline"
          >
            Open
            <ArrowUpRightIcon aria-hidden className="size-3" />
          </Link>
        )}
      </div>
      {error ? (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(error)}
        </p>
      ) : isPending ? (
        <Skeleton className="h-40 w-full rounded-lg" />
      ) : data.ties.length === 0 ? (
        <p className="rounded-lg border px-3 py-6 text-center text-[13px] text-muted-foreground">
          It is not tied to anything yet.
        </p>
      ) : (
        <ul className="space-y-3">
          {data.ties.map((tie) => (
            <Tie key={`${tie.label}:${tie.kind}`} tie={tie} onFollow={onFollow} />
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * Follows the ties of the workspace's things one at a time: pick a member, a team, an issue, see
 * what it is tied to, and step on to any of those. The trail of steps is kept so that each can
 * be gone back to.
 */
export function MapExplorer({
  workspaceId,
  kind,
  onKindChange,
}: {
  workspaceId: string;
  kind: MapKind;
  onKindChange: (kind: MapKind) => void;
}) {
  const [trail, setTrail] = useState<MapItem[]>([]);
  const [trailKind, setTrailKind] = useState(kind);
  // Another kind was picked: the trail of the last one no longer applies.
  if (trailKind !== kind) {
    setTrailKind(kind);
    setTrail([]);
  }
  const focus = trail.at(-1);
  const focusKind = focus ? followable(focus.kind) : null;
  return (
    <section aria-label="Follow the ties" className="space-y-3">
      <h2 className="text-[13px] font-medium">Follow the ties</h2>
      <div role="tablist" aria-label="Kind of thing" className="flex flex-wrap gap-1">
        {FOLLOWABLE.map((option) => (
          <Button
            key={option.kind}
            role="tab"
            aria-selected={option.kind === kind}
            variant={option.kind === kind ? 'secondary' : 'ghost'}
            size="sm"
            onClick={() => {
              onKindChange(option.kind);
              setTrail([]);
            }}
          >
            {option.label}
          </Button>
        ))}
      </div>
      {trail.length > 0 && (
        <nav aria-label="Steps taken" className="flex flex-wrap items-center gap-1 text-xs">
          <button
            type="button"
            className="text-muted-foreground underline-offset-2 hover:underline"
            onClick={() => setTrail([])}
          >
            {FOLLOWABLE.find((option) => option.kind === kind)?.label}
          </button>
          {trail.map((step, index) => (
            <span key={`${step.id}:${index}`} className="flex min-w-0 items-center gap-1">
              <ChevronRightIcon aria-hidden className="size-3 text-muted-foreground" />
              {index === trail.length - 1 ? (
                <span aria-current="step" className="max-w-48 truncate font-medium">
                  {step.title}
                </span>
              ) : (
                <button
                  type="button"
                  className="max-w-48 truncate text-muted-foreground underline-offset-2 hover:underline"
                  onClick={() => setTrail(trail.slice(0, index + 1))}
                >
                  {step.title}
                </button>
              )}
            </span>
          ))}
        </nav>
      )}
      {focus && focusKind ? (
        <Neighbourhood
          key={`${focusKind}:${focus.id}`}
          workspaceId={workspaceId}
          item={focus}
          kind={focusKind}
          onFollow={(item) => setTrail([...trail, item])}
        />
      ) : (
        <Picker
          key={kind}
          workspaceId={workspaceId}
          kind={kind}
          onPick={(item) => setTrail([item])}
        />
      )}
    </section>
  );
}
