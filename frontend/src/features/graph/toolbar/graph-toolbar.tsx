import { LayoutGridIcon, MaximizeIcon, PlusIcon, Trash2Icon } from 'lucide-react';

import type { Graph } from '@/schemas/graph';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { Separator } from '@/components/ui/separator';
import type { ConnectionState } from '@/lib/realtime/connection-state';
import { useGraphStore } from '@/stores/graph-store';

import { ConnectionIndicator } from './connection-indicator';
import { OntologyDialog } from './ontology-dialog';
import { PlanControls } from './plan-controls';
import { RunControls } from './run-controls';
import { ToolbarButton } from './toolbar-button';

interface GraphToolbarProps {
  graph: Graph;
  nodeCount: number;
  sse: ConnectionState;
  ws: ConnectionState;
  layoutPending: boolean;
  onAddNode: () => void;
  onAutoLayout: () => void;
  onFit: () => void;
  onDeleteEdge: (edgeId: string) => void;
  /** Called after a plan is applied (the workspace re-lays out the restructured graph). */
  onPlanApplied: () => void;
}

export function GraphToolbar({
  graph,
  nodeCount,
  sse,
  ws,
  layoutPending,
  onAddNode,
  onAutoLayout,
  onFit,
  onDeleteEdge,
  onPlanApplied,
}: GraphToolbarProps) {
  const selectedEdgeId = useGraphStore((s) => s.selectedEdgeId);
  const graphId = graph.id;
  return (
    <GlassCard
      role="toolbar"
      aria-label="Graph tools"
      className="pointer-events-auto flex max-w-full flex-wrap items-center gap-1 p-1 motion-safe:animate-fade-in"
    >
      <ToolbarButton icon={PlusIcon} label="Add node" shortcut="n" showLabel onClick={onAddNode} />
      <ToolbarButton
        icon={LayoutGridIcon}
        label="Auto-layout"
        shortcut="l"
        disabled={nodeCount === 0 || layoutPending}
        onClick={onAutoLayout}
      />
      <ToolbarButton icon={MaximizeIcon} label="Fit to view" shortcut="f" onClick={onFit} />
      <OntologyDialog graph={graph} />
      {selectedEdgeId && (
        <ToolbarButton
          icon={Trash2Icon}
          label="Delete edge"
          shortcut="delete"
          className="text-destructive"
          onClick={() => onDeleteEdge(selectedEdgeId)}
        />
      )}
      <Separator orientation="vertical" className="mx-0.5 data-[orientation=vertical]:h-5" />
      <PlanControls graphId={graphId} onApplied={onPlanApplied} />
      <RunControls graphId={graphId} disabled={nodeCount === 0} />
      <Separator orientation="vertical" className="mx-0.5 data-[orientation=vertical]:h-5" />
      <ConnectionIndicator sse={sse} ws={ws} />
    </GlassCard>
  );
}
