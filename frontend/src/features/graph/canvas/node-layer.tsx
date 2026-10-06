import { memo } from 'react';

import { NodeKindIcon } from '@/components/custom-ui/node-kind-icon';
import { useNodeType } from '@/components/custom-ui/node-kind-meta';
import { displayStatus, STATUS_META } from '@/components/custom-ui/status-meta';
import type { GraphNode, NodeStatus } from '@/schemas/graph';
import type { LiveNodeState } from '@/stores/graph-store';

import { NODE_HEIGHT, NODE_WIDTH, truncate } from './geometry';

interface NodeCardProps {
  id: string;
  title: string;
  kind: string;
  status: NodeStatus;
  cached: boolean;
  selected: boolean;
  onSelect: (id: string) => void;
}

const NodeCard = memo(function NodeCard({
  id,
  title,
  kind,
  status,
  cached,
  selected,
  onSelect,
}: NodeCardProps) {
  const shown = displayStatus(status, cached);
  const meta = STATUS_META[shown];
  const type = useNodeType(kind);
  return (
    <g
      className="gnode"
      data-node-id={id}
      data-status={status}
      data-selected={selected}
      style={{ '--node-status': meta.color } as React.CSSProperties}
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      aria-label={`${title} — ${type.label}, ${meta.label}`}
      onClick={() => onSelect(id)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onSelect(id);
        }
      }}
    >
      <title>{type.description ? `${title} — ${type.label}: ${type.description}` : title}</title>
      <rect
        className="gnode-ring"
        x={-3}
        y={-3}
        width={NODE_WIDTH + 6}
        height={NODE_HEIGHT + 6}
        rx={15}
      />
      <rect className="gnode-card" width={NODE_WIDTH} height={NODE_HEIGHT} rx={12} />
      <NodeKindIcon
        kind={kind}
        size={16}
        x={12}
        y={(NODE_HEIGHT - 16) / 2}
        className="gnode-icon"
      />
      <text className="gnode-title" x={38} y={22}>
        {truncate(title || 'Untitled', 21)}
      </text>
      <text className="gnode-sub" x={38} y={38}>
        {truncate(type.label, 14)} · {meta.label}
      </text>
      <circle className="gnode-dot" cx={NODE_WIDTH - 12} cy={12} r={3.5} />
    </g>
  );
});

interface NodeLayerProps {
  nodes: GraphNode[];
  liveStates: Record<string, LiveNodeState>;
  selectedNodeId: string | null;
  onSelect: (id: string) => void;
}

export function NodeLayer({ nodes, liveStates, selectedNodeId, onSelect }: NodeLayerProps) {
  return (
    <g>
      {nodes.map((node) => {
        const live = liveStates[node.id];
        return (
          <NodeCard
            key={node.id}
            id={node.id}
            title={node.title}
            kind={node.kind}
            status={live?.status ?? node.status}
            cached={live?.cached ?? false}
            selected={node.id === selectedNodeId}
            onSelect={onSelect}
          />
        );
      })}
    </g>
  );
}
