import { useInfiniteQuery } from '@tanstack/react-query';
import { SearchIcon, WaypointsIcon } from 'lucide-react';
import { useId, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { NativeSelect, NativeSelectOption } from '@/components/ui/native-select';
import { Skeleton } from '@/components/ui/skeleton';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { SPAN_KIND_META } from '@/features/runs/span-kinds';
import { errorMessage } from '@/lib/api/errors';
import { durationBetween, formatDateTime, formatDuration } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import { displayPrincipal, type Agent } from '@/schemas/agent';
import { DECISIONS, type Decision, type SecopsSpan, type SpanFilter } from '@/schemas/secops';
import { SPAN_KINDS, type SpanKind } from '@/schemas/trace';

import { SPANS_PAGE_SIZE, secopsSpansPage, spanFilterParams } from '../api';
import { localToIso, spanHref } from '../explorer';

function What({ span }: { span: SecopsSpan }) {
  const parts: string[] = [];
  if (span.tool && span.tool !== span.name) parts.push(span.tool);
  if (span.host) parts.push(`→ ${span.host}`);
  if (span.model && span.kind === 'llm_call') parts.push(span.model);
  if (span.decision)
    parts.push(span.decision === 'require_approval' ? 'waits for approval' : span.decision);
  if (span.rule) parts.push(span.rule);
  return <span className="truncate text-muted-foreground">{parts.join(' · ')}</span>;
}

interface TraceExplorerProps {
  workspaceId: string;
  agents: Agent[];
}

/**
 * Spans across every run the member may open, newest first, narrowed by agent, kind, tool,
 * host, run, time and decision. A row opens the run's waterfall at that span.
 */
export function TraceExplorer({ workspaceId, agents }: TraceExplorerProps) {
  const navigate = useNavigate();
  const ids = { tool: useId(), host: useId(), run: useId(), from: useId(), to: useId() };
  const [draft, setDraft] = useState<SpanFilter>({});
  const [filter, setFilter] = useState<SpanFilter>({});
  const params = spanFilterParams(filter);
  const query = useInfiniteQuery({
    queryKey: qk.secops.spans(workspaceId, params),
    queryFn: ({ pageParam, signal }) => secopsSpansPage(workspaceId, filter, pageParam, signal),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.next ?? undefined,
  });
  const spans = query.data?.pages.flatMap((p) => p.spans) ?? [];
  const names = new Map(agents.map((a) => [a.id, a.name]));
  const set = <K extends keyof SpanFilter>(key: K, value: SpanFilter[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));
  return (
    <div className="space-y-4">
      <form
        className="grid gap-2 rounded-xl border bg-card p-3 text-xs sm:grid-cols-2 lg:grid-cols-4 xl:grid-cols-8"
        aria-label="Filters"
        onSubmit={(e) => {
          e.preventDefault();
          setFilter(draft);
        }}
      >
        <label className="space-y-1">
          <span className="text-muted-foreground">Agent</span>
          <NativeSelect
            size="sm"
            className="w-full"
            value={draft.agent ?? ''}
            onChange={(e) => set('agent', e.target.value || undefined)}
          >
            <NativeSelectOption value="">any</NativeSelectOption>
            {agents.map((a) => (
              <NativeSelectOption key={a.id} value={a.id}>
                {a.name}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </label>
        <label className="space-y-1">
          <span className="text-muted-foreground">Kind</span>
          <NativeSelect
            size="sm"
            className="w-full"
            value={draft.kind ?? ''}
            onChange={(e) => set('kind', (e.target.value || undefined) as SpanKind | undefined)}
          >
            <NativeSelectOption value="">any</NativeSelectOption>
            {SPAN_KINDS.map((k) => (
              <NativeSelectOption key={k} value={k}>
                {SPAN_KIND_META[k].label}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </label>
        <label className="space-y-1">
          <span className="text-muted-foreground">Decision</span>
          <NativeSelect
            size="sm"
            className="w-full"
            value={draft.decision ?? ''}
            onChange={(e) => set('decision', (e.target.value || undefined) as Decision | undefined)}
          >
            <NativeSelectOption value="">any</NativeSelectOption>
            {DECISIONS.map((d) => (
              <NativeSelectOption key={d} value={d}>
                {d.replace('_', ' ')}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </label>
        <label htmlFor={ids.tool} className="space-y-1">
          <span className="text-muted-foreground">Tool</span>
          <Input
            id={ids.tool}
            value={draft.tool ?? ''}
            onChange={(e) => set('tool', e.target.value)}
            placeholder="make_docx"
            className="h-7 text-xs"
          />
        </label>
        <label htmlFor={ids.host} className="space-y-1">
          <span className="text-muted-foreground">Host</span>
          <Input
            id={ids.host}
            value={draft.host ?? ''}
            onChange={(e) => set('host', e.target.value)}
            placeholder="api.example.com"
            className="h-7 text-xs"
          />
        </label>
        <label htmlFor={ids.run} className="space-y-1">
          <span className="text-muted-foreground">Run id</span>
          <Input
            id={ids.run}
            value={draft.run ?? ''}
            onChange={(e) => set('run', e.target.value)}
            placeholder="0199…"
            className="h-7 font-mono text-xs"
          />
        </label>
        <label htmlFor={ids.from} className="space-y-1">
          <span className="text-muted-foreground">From</span>
          <Input
            id={ids.from}
            type="datetime-local"
            onChange={(e) => set('from', localToIso(e.target.value))}
            className="h-7 text-xs"
          />
        </label>
        <label htmlFor={ids.to} className="space-y-1">
          <span className="text-muted-foreground">To</span>
          <Input
            id={ids.to}
            type="datetime-local"
            onChange={(e) => set('to', localToIso(e.target.value))}
            className="h-7 text-xs"
          />
        </label>
        <div className="flex items-end gap-2 sm:col-span-2 lg:col-span-4 xl:col-span-8">
          <Button size="sm" type="submit">
            <SearchIcon />
            Search
          </Button>
          <Button
            size="sm"
            type="button"
            variant="ghost"
            onClick={() => {
              setDraft({});
              setFilter({});
            }}
          >
            Clear
          </Button>
          <span className="ml-auto text-muted-foreground tabular-nums" role="status">
            {query.isFetching && !query.isFetchingNextPage
              ? 'searching…'
              : `${spans.length}${query.hasNextPage ? '+' : ''} span${spans.length === 1 ? '' : 's'}`}
          </span>
        </div>
      </form>
      {query.isPending ? (
        <Skeleton className="h-48 rounded-xl" />
      ) : query.error ? (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(query.error)}
        </p>
      ) : spans.length === 0 ? (
        <EmptyState
          icon={WaypointsIcon}
          title="No span matches"
          description="Widen the filter, or run a graph: every model call, tool call, read, policy decision and approval lands here."
        />
      ) : (
        <div className="overflow-x-auto rounded-xl border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>When</TableHead>
                <TableHead>Kind</TableHead>
                <TableHead>Agent</TableHead>
                <TableHead>Name</TableHead>
                <TableHead>What</TableHead>
                <TableHead className="text-right">Tokens</TableHead>
                <TableHead className="text-right">Took</TableHead>
                <TableHead>Run</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {spans.map((span) => {
                const meta = SPAN_KIND_META[span.kind];
                const Icon = meta.icon;
                const error = span.status === 'error';
                const took = durationBetween(span.started_at, span.ended_at);
                return (
                  <TableRow
                    key={span.id}
                    className="cursor-pointer"
                    onClick={() => navigate(spanHref(span))}
                  >
                    <TableCell className="text-xs whitespace-nowrap text-muted-foreground tabular-nums">
                      <time dateTime={span.started_at}>{formatDateTime(span.started_at)}</time>
                    </TableCell>
                    <TableCell className="text-xs">
                      <span className="flex items-center gap-1.5">
                        <Icon className={cn('size-3.5', error && 'text-destructive')} aria-hidden />
                        {meta.label}
                      </span>
                    </TableCell>
                    <TableCell className="text-xs" title={span.principal}>
                      {span.agent_id
                        ? (names.get(span.agent_id) ?? displayPrincipal(span.principal))
                        : displayPrincipal(span.principal)}
                    </TableCell>
                    <TableCell className={cn('font-mono text-xs', error && 'text-destructive')}>
                      <Link
                        to={spanHref(span)}
                        className="hover:underline"
                        onClick={(e) => e.stopPropagation()}
                      >
                        {span.name}
                      </Link>
                    </TableCell>
                    <TableCell className="max-w-64 text-xs">
                      <What span={span} />
                    </TableCell>
                    <TableCell className="text-right text-xs tabular-nums">
                      {span.tokens_in !== null && span.tokens_out !== null
                        ? `${span.tokens_in} / ${span.tokens_out}`
                        : '—'}
                    </TableCell>
                    <TableCell className="text-right text-xs tabular-nums">
                      {took === null ? '—' : formatDuration(took)}
                    </TableCell>
                    <TableCell className="font-mono text-xs text-muted-foreground">
                      {span.run_id.slice(0, 8)}
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
          {query.hasNextPage && (
            <div className="border-t p-2">
              <Button
                size="sm"
                variant="outline"
                disabled={query.isFetchingNextPage}
                onClick={() => void query.fetchNextPage()}
              >
                Show {SPANS_PAGE_SIZE} older
              </Button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
