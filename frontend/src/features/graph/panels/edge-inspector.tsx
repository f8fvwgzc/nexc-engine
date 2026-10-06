import { ArrowRightIcon, SaveIcon, Trash2Icon, XIcon } from 'lucide-react';
import { useState } from 'react';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { relationLabel } from '@/components/custom-ui/node-kind-meta';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import type { Graph, GraphEdge } from '@/schemas/graph';
import { useGraphStore } from '@/stores/graph-store';

import { useUpdateEdge } from '../hooks/use-graph-mutations';

const REASON_MAX = 500;

function ReasonForm({ graphId, edge }: { graphId: string; edge: GraphEdge }) {
  const [reason, setReason] = useState(edge.reason);
  const { mutate, isPending } = useUpdateEdge(graphId);
  const dirty = reason.trim() !== edge.reason;
  return (
    <form
      className="flex items-center gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (dirty) mutate({ edgeId: edge.id, reason: reason.trim() });
      }}
    >
      <Input
        value={reason}
        maxLength={REASON_MAX}
        onChange={(e) => setReason(e.target.value)}
        aria-label="Why these nodes are related"
        placeholder="Why are these two related?"
        className="h-8 text-sm"
      />
      <Button type="submit" size="icon-sm" disabled={!dirty || isPending} aria-label="Save reason">
        <SaveIcon />
      </Button>
    </form>
  );
}

/** The selected edge: what relates the two nodes, and why. */
export function EdgeInspector({
  graph,
  onDelete,
}: {
  graph: Graph;
  onDelete: (edgeId: string) => void;
}) {
  const selectedEdgeId = useGraphStore((s) => s.selectedEdgeId);
  const selectEdge = useGraphStore((s) => s.selectEdge);
  const edge = graph.edges.find((e) => e.id === selectedEdgeId);
  if (!edge) return null;
  const title = (id: string) => graph.nodes.find((n) => n.id === id)?.title || 'Untitled';
  const relation = graph.ontology.relation_types.find((r) => r.key === edge.kind);
  return (
    <GlassCard
      role="region"
      aria-label="Edge inspector"
      className="pointer-events-auto w-full max-w-xl space-y-2 p-3 motion-safe:animate-fade-in"
    >
      <div className="flex items-center gap-2 text-sm">
        <span className="min-w-0 truncate font-medium">{title(edge.source)}</span>
        <ArrowRightIcon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
        <span className="min-w-0 truncate font-medium">{title(edge.target)}</span>
        <Badge variant="secondary" className="ml-auto shrink-0" title={relation?.description}>
          {relationLabel(relation, edge.kind)}
          {edge.blocking ? ' · blocking' : ''}
        </Badge>
        <Button
          variant="ghost"
          size="icon-sm"
          className="shrink-0 text-destructive"
          aria-label="Delete edge"
          onClick={() => onDelete(edge.id)}
        >
          <Trash2Icon />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          className="shrink-0"
          aria-label="Close edge inspector"
          onClick={() => selectEdge(null)}
        >
          <XIcon />
        </Button>
      </div>
      <ReasonForm key={`${edge.id}:${edge.reason}`} graphId={graph.id} edge={edge} />
    </GlassCard>
  );
}
