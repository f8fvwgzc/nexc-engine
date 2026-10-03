import { useQueryClient } from '@tanstack/react-query';
import { MoreHorizontalIcon, NetworkIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';
import { Link } from 'react-router-dom';

import { routeModules } from '@/app/route-modules';
import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { GlassCard } from '@/components/custom-ui/glass-card';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { formatRelative } from '@/lib/format';
import type { GraphSummary } from '@/schemas/graph';

import { graphQuery } from '../api';
import { useDeleteGraph } from '../hooks/use-graph-mutations';

export function GraphCard({ graph }: { graph: GraphSummary }) {
  const queryClient = useQueryClient();
  const deleteGraph = useDeleteGraph();
  const [confirming, setConfirming] = useState(false);
  const prefetch = () => {
    void queryClient.prefetchQuery(graphQuery(graph.id));
    void routeModules.graph();
  };

  return (
    <GlassCard interactive className="group h-full">
      <Link
        to={`/app/graphs/${graph.id}`}
        onMouseEnter={prefetch}
        onFocus={prefetch}
        className="flex h-full flex-col gap-3 rounded-[inherit] p-4 outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
      >
        <div className="flex items-start gap-3 pr-8">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-brand/20 to-brand-2/20 text-brand">
            <NetworkIcon className="size-4" aria-hidden />
          </span>
          <div className="min-w-0 space-y-0.5">
            <h3 className="truncate font-medium tracking-tight">{graph.name}</h3>
            <p className="line-clamp-2 text-sm text-muted-foreground">
              {graph.description || 'No description'}
            </p>
          </div>
        </div>
        <div className="mt-auto flex items-center gap-3 text-xs text-muted-foreground tabular-nums">
          <span>{graph.node_count} nodes</span>
          <span>{graph.edge_count} edges</span>
          <span className="ml-auto">Updated {formatRelative(graph.updated_at)}</span>
        </div>
      </Link>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="icon-sm"
            className="absolute top-3 right-3 opacity-70 group-hover:opacity-100 focus-visible:opacity-100"
            aria-label={`Actions for ${graph.name}`}
          >
            <MoreHorizontalIcon />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem variant="destructive" onSelect={() => setConfirming(true)}>
            <Trash2Icon />
            Delete graph
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        title={`Delete “${graph.name}”?`}
        description="All nodes, edges, runs and artifacts of this graph are permanently removed."
        confirmLabel="Delete graph"
        destructive
        onConfirm={() => deleteGraph.mutate(graph.id)}
      />
    </GlassCard>
  );
}
