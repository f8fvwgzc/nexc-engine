import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  BotIcon,
  CheckCircle2Icon,
  CircleDotIcon,
  CircleIcon,
  CircleXIcon,
  FileIcon,
  HandIcon,
  MinusCircleIcon,
  PlayIcon,
  RouteIcon,
  SparklesIcon,
  SquareIcon,
} from 'lucide-react';
import { useEffect, useEffectEvent, useMemo, useState } from 'react';
import { Link } from 'react-router-dom';

import { Markdown } from '@/components/custom-ui/markdown';
import { Button } from '@/components/ui/button';
import { CodeBlock, CodeBlockContent } from '@/components/ui/code-block';
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceDescription,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from '@/components/ui/trace';
import { applyPlan, requestPlan, startRun } from '@/features/graph/api';
import { graphQuery } from '@/features/graphs/api';
import { artifactsQuery, cancelRun, downloadArtifact, graphRunsQuery } from '@/features/runs/api';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { GraphEventStream } from '@/lib/realtime/sse';
import type { NodeStatus } from '@/schemas/graph';
import type { Issue, IssueEvent } from '@/schemas/issue';
import type { Plan } from '@/schemas/plan';
import type { SseEvent } from '@/schemas/realtime';
import { type Artifact, isRunActive, type Run } from '@/schemas/run';

import { createIssueGraph } from './api';

/** Most characters of a step's streamed output kept on screen. */
const OUTPUT_MAX_CHARS = 20_000;
/** Most log lines of a step kept on screen. */
const LOG_MAX_LINES = 40;

/** What one step of the latest run has shown since the page opened. */
interface Live {
  status?: NodeStatus;
  error?: string | null;
  output: string;
  logs: string[];
}

type TraceStatus = 'running' | 'error' | 'pending' | undefined;

/** How a step's status reads on a trace line: its marker, its icon and a word when needed. */
function stepLook(status: NodeStatus): {
  trace: TraceStatus;
  icon: React.ReactNode;
  note?: string;
} {
  switch (status) {
    case 'succeeded':
      return { trace: undefined, icon: <CheckCircle2Icon className="text-emerald-600" /> };
    case 'running':
      return { trace: 'running', icon: <CircleDotIcon /> };
    case 'waiting_approval':
      return { trace: 'running', icon: <HandIcon />, note: 'waiting for approval' };
    case 'failed':
      return { trace: 'error', icon: <CircleXIcon /> };
    case 'skipped':
      return { trace: 'pending', icon: <MinusCircleIcon />, note: 'skipped' };
    case 'cancelled':
      return { trace: 'pending', icon: <MinusCircleIcon />, note: 'stopped' };
    case 'queued':
    case 'idle':
      return { trace: 'pending', icon: <CircleIcon /> };
  }
}

/** One step of the plan: its line, and when opened what it streamed, logged and made. */
function Step({
  title,
  status,
  live,
  preview,
  error,
  files,
}: {
  title: string;
  status: NodeStatus;
  live: Live | undefined;
  preview: string | null;
  error: string | null;
  files: Artifact[];
}) {
  const look = stepLook(status);
  const running = status === 'running';
  // Opens while it streams, then follows the reader's clicks.
  const [open, setOpen] = useState<boolean | undefined>(undefined);
  const output = live?.output || preview || '';
  const logs = live?.logs ?? [];
  const failure = live?.error ?? error;
  const hasDetail = output || logs.length > 0 || failure || files.length > 0;
  const line = (
    <TraceLine status={look.trace}>
      <TraceIcon>{look.icon}</TraceIcon>
      <TraceTitle>{title}</TraceTitle>
      {look.note && <TraceDescription>{look.note}</TraceDescription>}
      {files.length > 0 && (
        <TraceDescription>
          {files.length} file{files.length === 1 ? '' : 's'}
        </TraceDescription>
      )}
    </TraceLine>
  );
  if (!hasDetail) return <div>{line}</div>;
  return (
    <TraceCollapsible open={open ?? running} onOpenChange={setOpen}>
      <TraceCollapsibleTrigger>{line}</TraceCollapsibleTrigger>
      <TraceCollapsibleContent className="space-y-2">
        {failure && <p className="text-sm text-destructive">{failure}</p>}
        {output && (
          <CodeBlock size="sm">
            <CodeBlockContent className="max-h-64" wrap>
              {output}
            </CodeBlockContent>
          </CodeBlock>
        )}
        {logs.length > 0 && (
          <CodeBlock size="sm">
            <CodeBlockContent className="max-h-40" wrap>
              {logs.join('\n')}
            </CodeBlockContent>
          </CodeBlock>
        )}
        {files.length > 0 && <FileList files={files} />}
      </TraceCollapsibleContent>
    </TraceCollapsible>
  );
}

/** Files a run made, each a download (a file the DLP classifier kept in says so). */
function FileList({ files }: { files: Artifact[] }) {
  return (
    <ul className="flex flex-wrap gap-1.5">
      {files.map((file) => (
        <li key={file.id}>
          <Button
            variant="outline"
            size="sm"
            className="h-7 gap-1.5 px-2 text-xs"
            disabled={file.quarantined_at !== null}
            title={file.quarantined_at ? `Held back: ${file.quarantine_reason}` : file.path}
            onClick={() => void downloadArtifact(file)}
          >
            <FileIcon className="size-3.5" />
            {file.path}
          </Button>
        </li>
      ))}
    </ul>
  );
}

/**
 * The work an issue hands to agents, live: the plan as a trace of steps, each with its status,
 * what it streams and the files it made, and the buttons that plan, run and stop it. Without a
 * graph yet, one button hands the issue over and asks for a plan.
 */
export function IssueRun({ issue }: { issue: Issue }) {
  const queryClient = useQueryClient();
  const graphId = issue.graph_id;
  const [plan, setPlan] = useState<Plan | null>(null);
  const [live, setLive] = useState<{ runId: string; steps: Record<string, Live> } | null>(null);

  const hand = useMutation({
    mutationFn: async () => {
      const linked = await createIssueGraph(issue.id);
      if (!linked.graph_id) throw new Error('the issue has no graph');
      return requestPlan(linked.graph_id);
    },
    onSuccess: (started) => {
      setPlan(started);
      void queryClient.invalidateQueries({ queryKey: qk.issues.all });
    },
  });

  if (!graphId) {
    return (
      <section aria-label="Agent work" className="space-y-2 border-t pt-4">
        <h2 className="text-[13px] font-medium text-muted-foreground">Agent work</h2>
        <p className="text-[13px] text-muted-foreground">
          Hand this issue to agents. They plan it as steps, run them, and post the result here for
          review.
        </p>
        <Button size="sm" disabled={hand.isPending} onClick={() => hand.mutate()}>
          <SparklesIcon />
          Hand to agents
        </Button>
        {hand.error && (
          <p role="alert" className="text-sm text-destructive">
            {errorMessage(hand.error)}
          </p>
        )}
      </section>
    );
  }
  return (
    <LiveRun
      issue={issue}
      graphId={graphId}
      plan={plan}
      setPlan={setPlan}
      live={live}
      setLive={setLive}
    />
  );
}

function LiveRun({
  issue,
  graphId,
  plan,
  setPlan,
  live,
  setLive,
}: {
  issue: Issue;
  graphId: string;
  plan: Plan | null;
  setPlan: React.Dispatch<React.SetStateAction<Plan | null>>;
  live: { runId: string; steps: Record<string, Live> } | null;
  setLive: React.Dispatch<
    React.SetStateAction<{ runId: string; steps: Record<string, Live> } | null>
  >;
}) {
  const queryClient = useQueryClient();
  const { data: graph } = useQuery(graphQuery(graphId));
  const { data: runs } = useQuery(graphRunsQuery(graphId));
  const run: Run | undefined = runs?.[0];
  const active = run ? isRunActive(run.status) : false;
  const { data: files = [] } = useQuery({
    ...artifactsQuery(run?.id ?? ''),
    enabled: Boolean(run),
  });

  // Reads current state, while the stream stays open once per graph.
  const onEvent = useEffectEvent((event: SseEvent) => {
    const step = (runId: string, nodeId: string, change: (s: Live) => Live) =>
      setLive((prev) => {
        const base = prev && prev.runId === runId ? prev : { runId, steps: {} };
        const current = base.steps[nodeId] ?? { output: '', logs: [] };
        return { runId, steps: { ...base.steps, [nodeId]: change(current) } };
      });
    switch (event.type) {
      case 'run.started':
        setLive({ runId: event.data.run.id, steps: {} });
        queryClient.setQueryData<Run[]>(qk.graphs.runs(graphId), (prev) => [
          event.data.run,
          ...(prev ?? []).filter((r) => r.id !== event.data.run.id),
        ]);
        break;
      case 'node.status':
        step(event.data.run_id, event.data.node_id, (s) => ({
          ...s,
          status: event.data.status,
          error: event.data.error,
        }));
        break;
      case 'node.output':
        step(event.data.run_id, event.data.node_id, (s) => ({
          ...s,
          output: (s.output + event.data.delta).slice(-OUTPUT_MAX_CHARS),
        }));
        break;
      case 'node.log':
        step(event.data.run_id, event.data.node_id, (s) => ({
          ...s,
          logs: [...s.logs, event.data.message].slice(-LOG_MAX_LINES),
        }));
        break;
      case 'artifact.created':
        void queryClient.invalidateQueries({
          queryKey: qk.runs.artifacts(event.data.artifact.run_id),
        });
        break;
      case 'run.finished':
        queryClient.setQueryData<Run[]>(qk.graphs.runs(graphId), (prev) =>
          (prev ?? []).map((r) => (r.id === event.data.run.id ? event.data.run : r)),
        );
        // The run reported back: a timeline entry, maybe a new state, an inbox entry.
        void queryClient.invalidateQueries({ queryKey: qk.issues.all });
        void queryClient.invalidateQueries({ queryKey: qk.runs.artifacts(event.data.run.id) });
        break;
      case 'plan.started':
        break;
      case 'plan.node':
        setPlan((prev) =>
          prev &&
          prev.id === event.data.plan_id &&
          !prev.nodes.some((n) => n.ref === event.data.node.ref)
            ? { ...prev, nodes: [...prev.nodes, event.data.node] }
            : prev,
        );
        break;
      case 'plan.ready':
        setPlan(event.data.plan);
        break;
      case 'plan.failed':
        setPlan((prev) =>
          prev && prev.id === event.data.plan_id
            ? { ...prev, status: 'failed', error: event.data.error }
            : prev,
        );
        break;
      default:
        break;
    }
  });

  useEffect(() => {
    const stream = new GraphEventStream(graphId, {
      onEvent: (event) => onEvent(event),
      // Anything missed while offline: the runs and the graph again.
      onReconnect: () => {
        void queryClient.invalidateQueries({ queryKey: qk.graphs.runs(graphId) });
        void queryClient.invalidateQueries({ queryKey: qk.graphs.detail(graphId) });
      },
    });
    stream.start();
    return () => stream.stop();
  }, [graphId, queryClient]);

  const askPlan = useMutation({
    mutationFn: () => requestPlan(graphId),
    onSuccess: setPlan,
  });
  const go = useMutation({
    mutationFn: async () => {
      if (plan?.status === 'ready') {
        await applyPlan(graphId, plan.id);
        setPlan(null);
        await queryClient.invalidateQueries({ queryKey: qk.graphs.detail(graphId) });
      }
      return startRun(graphId, {});
    },
    onSuccess: (started) => {
      setLive({ runId: started.id, steps: {} });
      queryClient.setQueryData<Run[]>(qk.graphs.runs(graphId), (prev) => [
        started,
        ...(prev ?? []).filter((r) => r.id !== started.id),
      ]);
    },
  });
  const stop = useMutation({
    mutationFn: () => (run ? cancelRun(run.id) : Promise.reject(new Error('nothing runs'))),
  });

  const titles = useMemo(
    () => new Map((graph?.nodes ?? []).map((n) => [n.id, n.title])),
    [graph?.nodes],
  );
  const steps = useMemo(() => {
    const ofRun = live && run && live.runId === run.id ? live.steps : {};
    if (run && run.node_runs.length > 0) {
      return run.node_runs.map((nr) => ({
        id: nr.node_id,
        title: titles.get(nr.node_id) ?? 'A step that was removed',
        status: ofRun[nr.node_id]?.status ?? nr.status,
        live: ofRun[nr.node_id],
        preview: nr.output_preview,
        error: nr.error,
      }));
    }
    return (graph?.nodes ?? []).map((n) => ({
      id: n.id,
      title: n.title,
      status: 'idle' as const,
      live: undefined,
      preview: null,
      error: null,
    }));
  }, [graph?.nodes, live, run, titles]);

  const planning = plan !== null && plan.status !== 'applied';
  const done = steps.filter((s) => s.status === 'succeeded').length;
  const error = askPlan.error ?? go.error ?? stop.error;

  return (
    <section aria-label="Agent work" className="space-y-3 border-t pt-4">
      <div className="flex items-center gap-2">
        <h2 className="text-[13px] font-medium text-muted-foreground">Agent work</h2>
        {run && (
          <Link
            to={`/app/runs/${run.id}`}
            className="text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
          >
            {active ? 'Started' : 'Last run'} {formatRelative(run.created_at)}
          </Link>
        )}
        <span className="ml-auto flex gap-1.5">
          {active ? (
            <Button
              variant="outline"
              size="sm"
              className="h-7"
              disabled={stop.isPending}
              onClick={() => stop.mutate()}
            >
              <SquareIcon />
              Stop
            </Button>
          ) : planning ? (
            <Button
              size="sm"
              className="h-7"
              disabled={plan.status !== 'ready' || go.isPending}
              onClick={() => go.mutate()}
            >
              <PlayIcon />
              Apply plan and run
            </Button>
          ) : steps.length === 0 ? (
            <Button
              size="sm"
              className="h-7"
              disabled={askPlan.isPending}
              onClick={() => askPlan.mutate()}
            >
              <SparklesIcon />
              Plan steps
            </Button>
          ) : (
            <Button size="sm" className="h-7" disabled={go.isPending} onClick={() => go.mutate()}>
              <PlayIcon />
              {run ? 'Run again' : 'Run'}
            </Button>
          )}
        </span>
      </div>

      {planning ? (
        <Trace>
          <TraceLine
            status={
              plan.status === 'streaming'
                ? 'running'
                : plan.status === 'failed'
                  ? 'error'
                  : undefined
            }
          >
            <TraceIcon>
              <RouteIcon />
            </TraceIcon>
            <TraceTitle>
              {plan.status === 'streaming'
                ? 'Planning the steps'
                : plan.status === 'failed'
                  ? 'The plan failed'
                  : 'Proposed plan'}
            </TraceTitle>
            <TraceDescription>
              {plan.nodes.length} step{plan.nodes.length === 1 ? '' : 's'}
            </TraceDescription>
          </TraceLine>
          {plan.error && <p className="text-sm text-destructive">{plan.error}</p>}
          {plan.summary && <p className="text-[13px] text-muted-foreground">{plan.summary}</p>}
          <Trace className="ml-6">
            {plan.nodes.map((node) => (
              <div key={node.ref}>
                <TraceLine status="pending">
                  <TraceIcon>
                    <CircleIcon />
                  </TraceIcon>
                  <TraceTitle>{node.title}</TraceTitle>
                  {node.agent_role && <TraceDescription>{node.agent_role}</TraceDescription>}
                </TraceLine>
              </div>
            ))}
          </Trace>
        </Trace>
      ) : steps.length === 0 ? (
        <p className="text-[13px] text-muted-foreground">
          No steps yet. Ask the agents for a plan, or open the graph to draw one.
        </p>
      ) : (
        <Trace>
          <TraceCollapsible defaultOpen>
            <TraceCollapsibleTrigger>
              <TraceLine
                status={active ? 'running' : run?.status === 'failed' ? 'error' : undefined}
              >
                <TraceIcon>
                  <RouteIcon />
                </TraceIcon>
                <TraceTitle>{graph?.name ?? issue.title}</TraceTitle>
                <TraceDescription>
                  {done} of {steps.length} done
                </TraceDescription>
              </TraceLine>
            </TraceCollapsibleTrigger>
            <TraceCollapsibleContent>
              <Trace>
                {steps.map((s) => (
                  <Step
                    key={s.id}
                    title={s.title}
                    status={s.status}
                    live={s.live}
                    preview={s.preview}
                    error={s.error}
                    files={files.filter((f) => f.node_id === s.id)}
                  />
                ))}
              </Trace>
            </TraceCollapsibleContent>
          </TraceCollapsible>
        </Trace>
      )}
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(error)}
        </p>
      )}
    </section>
  );
}

/**
 * A finished run's report on the issue's timeline: whether it worked, the deliverable it
 * produced, the files it made, and a link to the whole run.
 */
export function RunReport({ event }: { event: IssueEvent }) {
  const runId = event.run_id;
  const { data: files = [] } = useQuery({
    ...artifactsQuery(runId ?? ''),
    enabled: Boolean(runId),
  });
  const status = event.to ?? 'finished';
  const verb =
    status === 'succeeded'
      ? 'finished the work'
      : status === 'failed'
        ? 'could not finish the work'
        : 'were stopped';
  return (
    <li className="rounded-md border bg-card px-3 py-2">
      <div className="flex items-center gap-2 text-[13px]">
        <span
          aria-hidden
          className="flex size-5 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary"
        >
          <BotIcon className="size-3.5" />
        </span>
        <span className="font-medium">Agents</span>
        <span className={status === 'failed' ? 'text-destructive' : 'text-muted-foreground'}>
          {verb}
        </span>
        <span className="text-muted-foreground">{formatRelative(event.created_at)}</span>
        {runId && (
          <Link
            to={`/app/runs/${runId}`}
            className="ml-auto text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
          >
            Open run
          </Link>
        )}
      </div>
      {event.body && (
        // A deliverable can run long: it scrolls inside the entry, its headings sized for a timeline.
        <div className="mt-1.5 max-h-80 overflow-y-auto pr-1">
          <Markdown className="text-sm [&_h1]:text-base [&_h2]:text-sm [&_h3]:text-sm">
            {event.body}
          </Markdown>
        </div>
      )}
      {files.length > 0 && (
        <div className="mt-2">
          <FileList files={files} />
        </div>
      )}
    </li>
  );
}

/** The latest run of a sub-issue's graph, as one small trace line that moves while it runs. */
export function SubIssueRun({ graphId }: { graphId: string }) {
  const { data: runs } = useQuery(graphRunsQuery(graphId));
  const run = runs?.[0];
  if (!run) return null;
  const look =
    run.status === 'succeeded'
      ? { trace: undefined, icon: <CheckCircle2Icon className="text-emerald-600" />, word: 'done' }
      : run.status === 'failed'
        ? { trace: 'error' as const, icon: <CircleXIcon />, word: 'failed' }
        : run.status === 'cancelled'
          ? { trace: 'pending' as const, icon: <MinusCircleIcon />, word: 'stopped' }
          : { trace: 'running' as const, icon: <CircleDotIcon />, word: 'running' };
  return (
    <TraceLine status={look.trace} className="shrink-0 text-xs">
      <TraceIcon className="size-3.5 [&_svg:not([class*='size-'])]:size-3.5">{look.icon}</TraceIcon>
      <TraceTitle>{look.word}</TraceTitle>
    </TraceLine>
  );
}
