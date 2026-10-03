import { NodeKindIcon } from '@/components/custom-ui/node-kind-icon';
import type { PlanState } from '@/stores/graph-store';

import { NODE_HEIGHT, NODE_WIDTH, truncate, type Point } from './geometry';

/** Streamed plan proposals: dashed ghost cards that scale/fade in as they arrive. */
export function GhostLayer({
  plan,
  positions,
}: {
  plan: PlanState;
  positions: Map<string, Point>;
}) {
  const keyOf = (ref: string) => plan.nodes.find((n) => n.ref === ref)?.existing_id ?? ref;
  return (
    <g aria-hidden>
      {plan.edges.map((edge) => (
        <g
          key={`${edge.source_ref}->${edge.target_ref}`}
          data-link-source={keyOf(edge.source_ref)}
          data-link-target={keyOf(edge.target_ref)}
        >
          <path className="gghost-edge" markerEnd="url(#garrow-brand)" />
        </g>
      ))}
      {plan.nodes.map((node) => {
        const p = positions.get(node.ref);
        if (!p) return null;
        return (
          <g
            key={node.ref}
            transform={`translate(${p.x - NODE_WIDTH / 2},${p.y - NODE_HEIGHT / 2})`}
          >
            <g className="gghost" data-update={node.existing_id !== null}>
              <rect
                className="gghost-card"
                x={-4}
                y={-4}
                width={NODE_WIDTH + 8}
                height={NODE_HEIGHT + 8}
                rx={14}
              />
              {node.existing_id === null && (
                <>
                  <NodeKindIcon
                    kind={node.kind}
                    size={16}
                    x={12}
                    y={(NODE_HEIGHT - 16) / 2}
                    className="gnode-icon"
                  />
                  <text className="gghost-title" x={38} y={22}>
                    {truncate(node.title, 21)}
                  </text>
                </>
              )}
              <text className="gghost-sub" x={38} y={node.existing_id === null ? 38 : -10}>
                {node.existing_id === null ? 'proposed' : 'will update'}
              </text>
            </g>
          </g>
        );
      })}
    </g>
  );
}
