import { useQuery } from '@tanstack/react-query';
import { DownloadIcon, FileIcon } from 'lucide-react';
import { useEffect, useRef } from 'react';

import { Markdown } from '@/components/custom-ui/markdown';
import { CopyButton } from '@/components/custom-ui/copy-button';
import { StatusBadge } from '@/components/custom-ui/status-badge';
import { displayStatus } from '@/components/custom-ui/status-meta';
import { Button } from '@/components/ui/button';
import { artifactsQuery, downloadArtifact } from '@/features/runs/api';
import { errorMessage } from '@/lib/api/errors';
import { formatBytes, formatTokens } from '@/lib/format';
import { cn } from '@/lib/utils';
import type { GraphNode } from '@/schemas/graph';
import { useGraphStore } from '@/stores/graph-store';
import { toast } from 'sonner';

function useStickToBottom(dep: unknown) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
    if (nearBottom) el.scrollTop = el.scrollHeight;
  }, [dep]);
  return ref;
}

function NodeArtifacts({ runId, nodeId }: { runId: string; nodeId: string }) {
  const { data: artifacts = [] } = useQuery({
    ...artifactsQuery(runId),
    select: (list) => list.filter((a) => a.node_id === nodeId),
  });
  if (artifacts.length === 0) return null;
  return (
    <section className="space-y-2">
      <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
        Artifacts
      </h3>
      <ul className="space-y-1.5">
        {artifacts.map((artifact) => (
          <li key={artifact.id} className="flex items-center gap-2 rounded-lg border px-2.5 py-1.5">
            <FileIcon className="size-4 shrink-0 text-muted-foreground" aria-hidden />
            <span className="min-w-0 flex-1 truncate font-mono text-xs">{artifact.path}</span>
            <span className="text-xs text-muted-foreground">{formatBytes(artifact.size)}</span>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Download ${artifact.path}`}
              onClick={() => downloadArtifact(artifact).catch((e) => toast.error(errorMessage(e)))}
            >
              <DownloadIcon />
            </Button>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Live status, streamed output, logs, tokens and artifacts of a node in the current run. */
export function NodeRunOutput({ node }: { node: GraphNode }) {
  const run = useGraphStore((s) => s.run);
  const live = useGraphStore((s) => s.nodeStates[node.id]);
  const streamed = useGraphStore((s) => s.outputs[node.id]);
  const logs = useGraphStore((s) => s.logs[node.id]);
  const tokens = useGraphStore((s) => s.tokens[node.id]);
  const preview = run?.node_runs.find((nr) => nr.node_id === node.id)?.output_preview;
  const output = streamed || node.output || preview || '';
  const outputRef = useStickToBottom(output.length);
  const status = live?.status ?? node.status;

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <StatusBadge status={displayStatus(status, live?.cached ?? false)} />
        {live && live.attempt > 1 && <span>attempt {live.attempt}</span>}
        {tokens && (
          <span className="tabular-nums">
            {formatTokens(tokens.tokens_in)} in · {formatTokens(tokens.tokens_out)} out
          </span>
        )}
      </div>
      {live?.error && (
        <p
          role="alert"
          className="rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-sm whitespace-pre-wrap text-destructive"
        >
          {live.error}
        </p>
      )}
      <section className="space-y-2">
        <div className="flex items-center justify-between">
          <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
            Output
          </h3>
          {output && <CopyButton value={output} label="Copy output" />}
        </div>
        <div
          ref={outputRef}
          aria-live={status === 'running' ? 'polite' : undefined}
          className={cn(
            'max-h-96 min-h-24 overflow-auto rounded-lg border bg-muted/40 p-3 text-sm',
            !output && 'text-xs text-muted-foreground',
          )}
        >
          {output ? (
            <Markdown>{output}</Markdown>
          ) : status === 'running' ? (
            'Waiting for output…'
          ) : (
            'No output yet. Run the graph.'
          )}
          {status === 'running' && output && (
            <span className="ml-0.5 inline-block h-3.5 w-1.5 translate-y-0.5 bg-brand motion-safe:animate-pulse" />
          )}
        </div>
      </section>
      {logs && logs.length > 0 && (
        <section className="space-y-2">
          <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Log</h3>
          <ol className="max-h-48 space-y-0.5 overflow-auto rounded-lg border bg-muted/30 p-2 font-mono text-[11px]">
            {logs.map((line, i) => (
              <li key={i} className="flex gap-2">
                <span
                  className={cn(
                    'w-10 shrink-0 uppercase',
                    line.level === 'error' ? 'text-destructive' : 'text-muted-foreground',
                  )}
                >
                  {line.level}
                </span>
                <span className="break-words whitespace-pre-wrap">{line.message}</span>
              </li>
            ))}
          </ol>
        </section>
      )}
      {run && <NodeArtifacts runId={run.id} nodeId={node.id} />}
    </div>
  );
}
