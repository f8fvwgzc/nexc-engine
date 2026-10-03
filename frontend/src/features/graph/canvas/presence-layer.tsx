import type { Peer } from '@/stores/graph-store';

/** Other collaborators' cursors (graph coordinates, sent over the WebSocket). */
export function PresenceLayer({ peers }: { peers: Record<string, Peer> }) {
  return (
    <g aria-hidden>
      {Object.entries(peers).map(([userId, peer]) =>
        peer.cursor ? (
          <g
            key={userId}
            className="gpresence"
            transform={`translate(${peer.cursor.x},${peer.cursor.y})`}
            style={{ transition: 'transform 120ms linear' }}
          >
            <circle r={5} />
            <text x={9} y={4}>
              {peer.name}
            </text>
          </g>
        ) : null,
      )}
    </g>
  );
}
