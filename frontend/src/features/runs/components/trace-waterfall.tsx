import { BotIcon, ChevronRightIcon, WaypointsIcon } from 'lucide-react';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { formatDuration } from '@/lib/format';
import { cn } from '@/lib/utils';
import type { Span } from '@/schemas/trace';

import { SPAN_KIND_META } from '../span-kinds';
import {
  barOf,
  buildTraceTree,
  describeSpan,
  durationOf,
  traceWindow,
  type TraceRow,
  type TraceWindow,
} from '../trace-tree';

const ROW =
  'grid grid-cols-[minmax(0,1fr)_4.5rem] items-center gap-3 px-3 py-1.5 sm:grid-cols-[minmax(0,20rem)_minmax(0,1fr)_4.5rem]';

function SpanLine({ span, window, depth }: { span: Span; window: TraceWindow; depth: number }) {
  const meta = SPAN_KIND_META[span.kind];
  const Icon = meta.icon;
  const bar = barOf(span, window);
  const duration = durationOf(span);
  const detail = describeSpan(span);
  const error = span.status === 'error';
  return (
    <li className={ROW}>
      <div
        className="flex min-w-0 items-center gap-2"
        style={{ paddingLeft: `${depth * 1.25}rem` }}
      >
        <Icon
          className={cn('size-3.5 shrink-0 text-muted-foreground', error && 'text-destructive')}
          aria-label={meta.label}
        />
        <span className={cn('truncate font-mono text-xs', error && 'text-destructive')}>
          {span.name}
        </span>
        {detail && <span className="truncate text-xs text-muted-foreground">{detail}</span>}
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
    </li>
  );
}

function Rows({ rows, window, depth }: { rows: TraceRow[]; window: TraceWindow; depth: number }) {
  return rows.map((row) => <Branch key={row.span.id} row={row} window={window} depth={depth} />);
}

function Branch({ row, window, depth }: { row: TraceRow; window: TraceWindow; depth: number }) {
  return (
    <>
      <SpanLine span={row.span} window={window} depth={depth} />
      <Rows rows={row.children} window={window} depth={depth + 1} />
    </>
  );
}

interface TraceWaterfallProps {
  spans: Span[];
  titles: Map<string, string>;
  /** The run is still going: spans keep arriving. */
  live: boolean;
}

/** The run's trace as a waterfall, grouped node → agent → turn → tool call. */
export function TraceWaterfall({ spans, titles, live }: TraceWaterfallProps) {
  if (spans.length === 0) {
    return (
      <EmptyState
        icon={WaypointsIcon}
        title={live ? 'Waiting for the first span' : 'No trace'}
        description={
          live
            ? 'Model calls, tool calls and sub-agents appear here as the agents work.'
            : 'This run recorded no spans: its nodes were served from cache, or it ran before tracing existed.'
        }
      />
    );
  }
  const window = traceWindow(spans);
  const nodes = buildTraceTree(spans);
  return (
    <div className="divide-y overflow-hidden rounded-xl border" aria-live={live ? 'polite' : 'off'}>
      {nodes.map((node) => (
        <Collapsible key={node.nodeId} defaultOpen>
          <CollapsibleTrigger className="group flex w-full items-center gap-2 bg-muted/30 px-3 py-2 text-left text-sm outline-none hover:bg-muted/50 focus-visible:ring-3 focus-visible:ring-ring/50">
            <ChevronRightIcon
              className="size-4 shrink-0 text-muted-foreground transition-transform group-data-[state=open]:rotate-90"
              aria-hidden
            />
            <span className="truncate font-medium">
              {titles.get(node.nodeId) ?? node.nodeId.slice(0, 8)}
            </span>
            <span className="ml-auto shrink-0 text-xs text-muted-foreground tabular-nums">
              {node.spans} spans
              {node.errors > 0 && (
                <span className="text-destructive"> · {node.errors} failed</span>
              )}{' '}
              · {formatDuration(node.end - node.start)}
            </span>
          </CollapsibleTrigger>
          <CollapsibleContent>
            <ul className="py-1">
              {node.context.map((span) => (
                <SpanLine key={span.id} span={span} window={window} depth={0} />
              ))}
            </ul>
            {node.agents.map((agent) => (
              <div key={agent.principal} className="border-t border-dashed">
                <p className="flex items-center gap-1.5 px-3 pt-2 text-xs font-medium text-muted-foreground">
                  <BotIcon className="size-3.5" aria-hidden />
                  {agent.principal}
                </p>
                <ul className="pb-1">
                  <Rows rows={agent.rows} window={window} depth={0} />
                </ul>
              </div>
            ))}
          </CollapsibleContent>
        </Collapsible>
      ))}
    </div>
  );
}
