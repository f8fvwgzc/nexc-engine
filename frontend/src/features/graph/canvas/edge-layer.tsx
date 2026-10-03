import { memo } from 'react';

import type { EdgeSuggestion, GraphEdge } from '@/schemas/graph';

interface EdgeViewProps {
  edge: GraphEdge;
  selected: boolean;
  onSelect: (id: string) => void;
}

const EdgeView = memo(function EdgeView({ edge, selected, onSelect }: EdgeViewProps) {
  const marker =
    edge.kind === 'depends_on' ? `url(#${selected ? 'garrow-active' : 'garrow'})` : undefined;
  return (
    <g
      className="gedge"
      data-link-source={edge.source}
      data-link-target={edge.target}
      data-edge-id={edge.id}
      data-kind={edge.kind}
      data-origin={edge.origin}
      data-selected={selected}
      onClick={() => onSelect(edge.id)}
    >
      <title>
        {edge.kind === 'depends_on' ? 'Depends on' : 'Related'}
        {edge.origin !== 'user' ? ` (${edge.origin})` : ''}
      </title>
      <path className="gedge-hit" />
      <path className="gedge-line" markerEnd={marker} />
    </g>
  );
});

export function EdgeLayer({
  edges,
  selectedEdgeId,
  onSelect,
}: {
  edges: GraphEdge[];
  selectedEdgeId: string | null;
  onSelect: (id: string) => void;
}) {
  return (
    <g>
      {edges.map((edge) => (
        <EdgeView
          key={edge.id}
          edge={edge}
          selected={edge.id === selectedEdgeId}
          onSelect={onSelect}
        />
      ))}
    </g>
  );
}

/** Auto-detected dependency candidates: dashed + animated; click to accept as a depends_on edge. */
export function SuggestionLayer({
  suggestions,
  onAccept,
}: {
  suggestions: EdgeSuggestion[];
  onAccept: (s: EdgeSuggestion) => void;
}) {
  return (
    <g>
      {suggestions.map((s) => (
        <g
          key={`${s.source}->${s.target}`}
          className="gsuggest"
          data-link-source={s.source}
          data-link-target={s.target}
          data-suggestion
          role="button"
          tabIndex={0}
          aria-label={`Suggested dependency: ${s.reason}. Activate to accept.`}
          onClick={() => onAccept(s)}
          onKeyDown={(e) => e.key === 'Enter' && onAccept(s)}
        >
          <title>{`Suggested (${Math.round(s.score * 100)}%): ${s.reason} — click to accept`}</title>
          <path className="gedge-hit" />
          <path className="gsuggest-line" markerEnd="url(#garrow-brand)" />
        </g>
      ))}
    </g>
  );
}
