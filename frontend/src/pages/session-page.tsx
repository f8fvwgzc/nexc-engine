import { useQuery } from '@tanstack/react-query';
import {
  ArrowLeftIcon,
  BotIcon,
  ChevronsDownUpIcon,
  ChevronsUpDownIcon,
  ListTodoIcon,
  ListTreeIcon,
  PlayIcon,
} from 'lucide-react';
import { useMemo, useState } from 'react';
import { Link, useParams, useSearchParams } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { StatusBadge } from '@/components/custom-ui/status-badge';
import { STATUS_META } from '@/components/custom-ui/status-meta';
import { ContentSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { NativeSelect, NativeSelectOption } from '@/components/ui/native-select';
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable';
import { agentsQuery } from '@/features/agents/api';
import { runTraceQuery } from '@/features/runs/api';
import { useCancelRun } from '@/features/runs/hooks/use-cancel-run';
import { SPAN_KIND_META } from '@/features/runs/span-kinds';
import { sessionQuery } from '@/features/sessions/api';
import { SessionHeader } from '@/features/sessions/components/session-header';
import { SessionTree } from '@/features/sessions/components/session-tree';
import { SpanTimeline } from '@/features/sessions/components/span-timeline';
import { useCancelAgent } from '@/features/sessions/hooks/use-cancel-agent';
import { useLiveSession } from '@/features/sessions/hooks/use-live-session';
import {
  allKeys,
  filterRows,
  flattenSession,
  spansOfRow,
  tokensLabel,
  type TreeFilter,
  type TreeRow,
} from '@/features/sessions/tree';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { useIsMobile } from '@/hooks/use-mobile';
import { ApiError } from '@/lib/api/errors';
import { formatCost, formatDuration } from '@/lib/format';
import { isRunActive } from '@/schemas/run';
import { SESSION_STATUSES, type SessionStatus } from '@/schemas/session';
import { SPAN_KINDS, type SpanKind } from '@/schemas/trace';

const NO_FILTER: TreeFilter = { agent: 'all', kind: 'all', status: 'all' };
const ROOT_ONLY = new Set(['run']);

const ROW_ICONS = { run: PlayIcon, node: ListTodoIcon, agent: BotIcon } as const;

/** The selected row in one card: what it is, how it ended, what it spent. */
function RowSummary({ row }: { row: TreeRow }) {
  const Icon = ROW_ICONS[row.kind];
  const line = (rollup: TreeRow['rollup']) =>
    `${tokensLabel(rollup)} · ${formatCost(rollup.cost_usd)} · ${rollup.llm_calls} turns · ${rollup.tool_calls} tool calls` +
    (rollup.denials > 0 ? ` · ${rollup.denials} denied` : '');
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-xl border bg-card px-3 py-2 text-sm">
      <Icon className="size-4 text-muted-foreground" aria-hidden />
      <span className="font-medium">{row.title}</span>
      <span className="text-muted-foreground">{row.subtitle}</span>
      <StatusBadge status={row.status} />
      <span className="text-xs text-muted-foreground tabular-nums">
        {row.durationMs === null ? 'no duration yet' : formatDuration(row.durationMs)}
      </span>
      <p className="basis-full text-xs text-muted-foreground tabular-nums">
        {row.own ? 'Subtree: ' : ''}
        {line(row.rollup)}
      </p>
      {row.own && (
        <p className="basis-full text-xs text-muted-foreground tabular-nums">
          Own: {line(row.own)}
        </p>
      )}
      {row.error && (
        <p role="alert" className="basis-full text-xs text-destructive">
          {row.error}
        </p>
      )}
    </div>
  );
}

function SessionView({ runId }: { runId: string }) {
  const [params] = useSearchParams();
  // `?span=` opens the outline at the row holding that span (the console and explorer link here).
  const highlight = params.get('span');
  const session = useQuery(sessionQuery(runId));
  const trace = useQuery(runTraceQuery(runId));
  useLiveSession(session.data);
  const { current } = useCurrentWorkspace();
  const { data: agents = [] } = useQuery({
    ...agentsQuery(current?.id),
    enabled: current !== undefined,
  });
  const isMobile = useIsMobile();
  // Until the viewer folds or selects, the whole outline is open and `?span=` names the row.
  const [expandedByViewer, setExpanded] = useState<Set<string> | null>(null);
  const [selectedByViewer, setSelected] = useState<string | null>(null);
  const [drilled, setDrilled] = useState(highlight !== null);
  const [filter, setFilter] = useState<TreeFilter>(NO_FILTER);
  const cancelAgent = useCancelAgent(runId);
  const cancelRun = useCancelRun();

  const data = session.data;
  const spans = useMemo(() => trace.data ?? [], [trace.data]);
  const spansById = useMemo(() => new Map(spans.map((s) => [s.id, s])), [spans]);

  const allOpen = useMemo(() => (data ? allKeys(data) : ROOT_ONLY), [data]);
  const expanded = expandedByViewer ?? allOpen;
  const highlightRow = useMemo(() => {
    if (!data || !highlight) return null;
    const row = flattenSession(data, allOpen).find(
      (r) => r.spanIds !== 'all' && r.spanIds.includes(highlight),
    );
    return row?.key ?? null;
  }, [data, allOpen, highlight]);
  const selected = selectedByViewer ?? highlightRow ?? 'run';

  const rows = useMemo(
    () => (data ? filterRows(flattenSession(data, expanded), filter, spansById) : []),
    [data, expanded, filter, spansById],
  );
  const selectedRow = rows.find((r) => r.key === selected) ?? rows[0] ?? null;

  if (session.isPending) return <ContentSkeleton />;
  // A missing run is not an outage: the first gets a way back, the second a retry.
  const missing = session.error instanceof ApiError && session.error.status === 404;
  if (session.isError && !missing) {
    return (
      <ErrorState
        title="Couldn’t load this session"
        error={session.error}
        onRetry={() => void session.refetch()}
      />
    );
  }
  if (!data) {
    return (
      <EmptyState
        icon={ListTreeIcon}
        title="This session cannot be opened"
        description="The run was not found: it may have been deleted, or it belongs to another workspace."
        action={
          <Button variant="outline" asChild>
            <Link to="/app/sessions">
              <ArrowLeftIcon />
              All sessions
            </Link>
          </Button>
        }
      />
    );
  }

  const active = isRunActive(data.status);
  const canStop = current !== undefined && current.role !== 'guest';
  const toggle = (key: string) =>
    setExpanded((prev) => {
      const next = new Set(prev ?? allOpen);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  const select = (key: string) => {
    setSelected(key);
    if (isMobile) setDrilled(true);
  };
  const setPart = <K extends keyof TreeFilter>(key: K, value: TreeFilter[K]) =>
    setFilter((f) => ({ ...f, [key]: value }));

  const filters = (
    <div
      role="group"
      aria-label="Narrow the outline"
      className="flex flex-wrap items-end gap-2 text-xs"
    >
      <label className="space-y-1">
        <span className="text-muted-foreground">Agent</span>
        <NativeSelect
          size="sm"
          className="w-full"
          value={filter.agent}
          onChange={(e) => setPart('agent', e.target.value)}
        >
          <NativeSelectOption value="all">any agent</NativeSelectOption>
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
          value={filter.kind}
          onChange={(e) => setPart('kind', e.target.value as 'all' | SpanKind)}
        >
          <NativeSelectOption value="all">any kind</NativeSelectOption>
          {SPAN_KINDS.map((k) => (
            <NativeSelectOption key={k} value={k}>
              {SPAN_KIND_META[k].label}
            </NativeSelectOption>
          ))}
        </NativeSelect>
      </label>
      <label className="space-y-1">
        <span className="text-muted-foreground">Status</span>
        <NativeSelect
          size="sm"
          className="w-full"
          value={filter.status}
          onChange={(e) => setPart('status', e.target.value as 'all' | SessionStatus)}
        >
          <NativeSelectOption value="all">any status</NativeSelectOption>
          {SESSION_STATUSES.map((s) => (
            <NativeSelectOption key={s} value={s}>
              {STATUS_META[s].label}
            </NativeSelectOption>
          ))}
        </NativeSelect>
      </label>
      <div className="ml-auto flex gap-1">
        <Button variant="ghost" size="sm" onClick={() => setExpanded(allOpen)}>
          <ChevronsUpDownIcon />
          Expand all
        </Button>
        <Button variant="ghost" size="sm" onClick={() => setExpanded(new Set(ROOT_ONLY))}>
          <ChevronsDownUpIcon />
          Collapse all
        </Button>
      </div>
    </div>
  );

  const tree = (
    <SessionTree
      rows={rows}
      expanded={expanded}
      onToggle={toggle}
      selected={selectedRow?.key ?? null}
      onSelect={select}
      canStop={canStop}
      runActive={active}
      onStopAgent={(row) => {
        if (row.cancellable) cancelAgent.mutate({ spawnSpanId: row.cancellable });
      }}
      onStopRun={() => cancelRun.mutate(runId)}
    />
  );

  const details = selectedRow && (
    <div className="space-y-3">
      <RowSummary row={selectedRow} />
      {trace.isError && (
        <ErrorState
          compact
          title="Couldn’t load the spans"
          error={trace.error}
          onRetry={() => void trace.refetch()}
        />
      )}
      <SpanTimeline
        title={selectedRow.title}
        spans={spansOfRow(selectedRow, spans)}
        recording={data.recording}
        highlight={highlight}
        live={active}
      />
    </div>
  );

  if (isMobile) {
    return (
      <div className="space-y-3">
        <SessionHeader session={data} />
        {drilled && details ? (
          <div className="space-y-3">
            <Button variant="outline" size="sm" onClick={() => setDrilled(false)}>
              <ArrowLeftIcon />
              Back to the outline
            </Button>
            {details}
          </div>
        ) : (
          <>
            {filters}
            {tree}
          </>
        )}
      </div>
    );
  }

  return (
    <div className="space-y-3">
      <SessionHeader session={data} />
      {filters}
      <ResizablePanelGroup orientation="horizontal" className="min-h-96 items-start">
        <ResizablePanel defaultSize="55" minSize="30" className="pr-3">
          {tree}
        </ResizablePanel>
        <ResizableHandle withHandle />
        <ResizablePanel defaultSize="45" minSize="25" className="pl-3">
          {details}
        </ResizablePanel>
      </ResizablePanelGroup>
    </div>
  );
}

export default function SessionPage() {
  const { runId = '' } = useParams();
  const short = runId.slice(0, 8);
  return (
    <PageContainer width="wide">
      <Seo title={`Session ${short}`} noIndex />
      <PageHeader
        title={`Session ${short}`}
        description="The run as an outline: its nodes, the agent each ran as, the sub-agents they spawned; a row opens its spans."
        actions={
          <Button asChild variant="outline" size="sm">
            <Link to="/app/sessions">
              <ListTreeIcon />
              All sessions
            </Link>
          </Button>
        }
      />
      <SessionView key={runId} runId={runId} />
    </PageContainer>
  );
}
