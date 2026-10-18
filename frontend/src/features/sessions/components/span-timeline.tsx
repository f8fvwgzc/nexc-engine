import { useQuery } from '@tanstack/react-query';
import { ChevronRightIcon, FileTextIcon, SettingsIcon, WaypointsIcon } from 'lucide-react';
import { useState } from 'react';
import { Link } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { SPAN_KIND_META } from '@/features/runs/span-kinds';
import { barOf, describeSpan, durationOf, traceWindow } from '@/features/runs/trace-tree';
import { ApiError, errorMessage } from '@/lib/api/errors';
import { formatDuration } from '@/lib/format';
import { cn } from '@/lib/utils';
import type { Span } from '@/schemas/trace';

import { spanPayloadQuery } from '../api';
import { PayloadViewer } from './payload-viewer';

/** Most spans drawn at once (a run row may name thousands). */
const SPANS_MAX = 300;

/** The attributes a span carries, as digests and counts: what is there without recording. */
function Digests({ span }: { span: Span }) {
  const entries = Object.entries(span.attributes).filter(
    ([, v]) => typeof v !== 'object' || v === null,
  );
  if (entries.length === 0) return null;
  return (
    <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5 text-[11px]">
      {entries.map(([key, value]) => (
        <div key={key} className="contents">
          <dt className="text-muted-foreground">{key}</dt>
          <dd className="truncate font-mono" title={String(value)}>
            {String(value)}
          </dd>
        </div>
      ))}
    </dl>
  );
}

/** What a span said, when the workspace records it. */
function Payloads({ span }: { span: Span }) {
  const query = useQuery(spanPayloadQuery(span.id));
  if (query.isPending) return <Skeleton className="h-16 rounded-lg" />;
  if (query.isError) {
    if (query.error instanceof ApiError && query.error.status === 403) {
      return (
        <p className="text-xs text-muted-foreground">
          Only the run&apos;s owner and the workspace&apos;s admins read what a span said.
        </p>
      );
    }
    return (
      <p role="alert" className="text-xs text-destructive">
        {errorMessage(query.error)}
      </p>
    );
  }
  const { labels, payloads } = query.data;
  if (payloads.length === 0) {
    return (
      <p className="text-xs text-muted-foreground">
        Nothing was recorded for this span: recording was off, or it is a kind that records nothing.
      </p>
    );
  }
  return (
    <div className="space-y-2">
      {labels.length > 0 && (
        <p className="flex flex-wrap items-center gap-1.5 text-xs">
          <span className="text-muted-foreground">Derived from</span>
          {labels.map((label) => (
            <Badge key={label} variant="outline" className="capitalize">
              {label}
            </Badge>
          ))}
          <span className="text-muted-foreground">data</span>
        </p>
      )}
      {payloads.map((payload) => (
        <PayloadViewer key={payload.kind} payload={payload} />
      ))}
    </div>
  );
}

function SpanRow({
  span,
  window,
  recording,
  highlighted,
}: {
  span: Span;
  window: ReturnType<typeof traceWindow>;
  recording: boolean;
  highlighted: boolean;
}) {
  const [open, setOpen] = useState(highlighted);
  const meta = SPAN_KIND_META[span.kind];
  const Icon = meta.icon;
  const bar = barOf(span, window);
  const duration = durationOf(span);
  const detail = describeSpan(span);
  const error = span.status === 'error';
  const recordable = span.kind === 'llm_call' || span.kind === 'tool_call';
  return (
    <li
      id={`span-${span.id}`}
      className={cn('px-2 py-1', highlighted && 'bg-brand/10 ring-1 ring-brand/40 ring-inset')}
    >
      <div className="grid grid-cols-[auto_minmax(0,1fr)_4rem] items-center gap-2 sm:grid-cols-[auto_minmax(0,16rem)_minmax(0,1fr)_4rem]">
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-expanded={open}
          aria-label={open ? `Fold ${span.name}` : `Unfold ${span.name}`}
          onClick={() => setOpen((o) => !o)}
        >
          <ChevronRightIcon className={cn('transition-transform', open && 'rotate-90')} />
        </Button>
        <div className="flex min-w-0 items-center gap-1.5">
          <Icon
            className={cn('size-3.5 shrink-0 text-muted-foreground', error && 'text-destructive')}
            aria-label={meta.label}
          />
          <span className={cn('truncate font-mono text-xs', error && 'text-destructive')}>
            {span.name}
          </span>
          {detail && (
            <span className="hidden truncate text-xs text-muted-foreground md:inline">
              {detail}
            </span>
          )}
        </div>
        <div className="relative hidden h-2 rounded-full bg-muted sm:block" aria-hidden>
          <span
            className={cn('absolute inset-y-0 rounded-full', error ? 'bg-destructive' : meta.bar)}
            style={{ left: `${bar.left}%`, width: `${bar.width}%` }}
          />
        </div>
        <span className="text-right text-xs text-muted-foreground tabular-nums">
          {duration === null ? '—' : formatDuration(duration)}
        </span>
      </div>
      {open && (
        <div className="mt-1 space-y-2 border-l pl-3 sm:ml-8">
          <Digests span={span} />
          {recordable &&
            (recording ? (
              <Payloads span={span} />
            ) : (
              <p className="flex flex-wrap items-center gap-1 text-xs text-muted-foreground">
                <FileTextIcon className="size-3.5" aria-hidden />
                Digests only: this run was not recorded.
                <Link
                  to="/app/settings/guardrails"
                  className="inline-flex items-center gap-1 font-medium underline-offset-4 hover:underline"
                >
                  <SettingsIcon className="size-3.5" aria-hidden />
                  Enable recording
                </Link>
                to keep prompts, completions and tool calls of the next runs.
              </p>
            ))}
        </div>
      )}
    </li>
  );
}

interface SpanTimelineProps {
  title: string;
  spans: Span[];
  /** Whether the run was recorded: payloads exist to read. */
  recording: boolean;
  /** A span to mark and open (`?span=`). */
  highlight?: string | null;
  live: boolean;
}

/**
 * The spans of the selected row as a timeline, each opening on its digests and, when the run
 * was recorded, on what it said.
 */
export function SpanTimeline({
  title,
  spans,
  recording,
  highlight = null,
  live,
}: SpanTimelineProps) {
  if (spans.length === 0) {
    return (
      <EmptyState
        icon={WaypointsIcon}
        title={live ? 'Waiting for the first span' : 'No spans'}
        description={
          live
            ? 'Model turns, tool calls and sub-agents appear here as the agents work.'
            : 'This part of the run recorded no spans.'
        }
      />
    );
  }
  const sorted = [...spans].sort(
    (a, b) => Date.parse(a.started_at) - Date.parse(b.started_at) || a.seq - b.seq,
  );
  const shown = sorted.slice(0, SPANS_MAX);
  const window = traceWindow(shown);
  return (
    <section aria-label={`Spans of ${title}`} className="rounded-xl border bg-card">
      <header className="flex items-center gap-2 border-b px-3 py-2 text-sm">
        <WaypointsIcon className="size-4 text-muted-foreground" aria-hidden />
        <span className="truncate font-medium">{title}</span>
        <span className="ml-auto shrink-0 text-xs text-muted-foreground tabular-nums">
          {spans.length} span{spans.length === 1 ? '' : 's'}
          {spans.length > SPANS_MAX ? ` · first ${SPANS_MAX} shown` : ''}
        </span>
      </header>
      <ul className="divide-y" aria-live={live ? 'polite' : 'off'}>
        {shown.map((span) => (
          <SpanRow
            key={span.id}
            span={span}
            window={window}
            recording={recording}
            highlighted={span.id === highlight}
          />
        ))}
      </ul>
    </section>
  );
}
