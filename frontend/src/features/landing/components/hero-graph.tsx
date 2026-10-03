import '@/styles/landing.css';

interface MiniNode {
  id: string;
  x: number;
  y: number;
  title: string;
  sub: string;
  ghost?: boolean;
}

const NODES: MiniNode[] = [
  { id: 'goal', x: 40, y: 130, title: 'Research report', sub: 'topic' },
  { id: 'sources', x: 200, y: 50, title: 'Find sources', sub: 'research' },
  { id: 'outline', x: 200, y: 210, title: 'Outline', sub: 'task' },
  { id: 'draft', x: 360, y: 130, title: 'Draft sections', sub: 'document' },
  { id: 'cite', x: 360, y: 270, title: 'Citations', sub: 'proposed', ghost: true },
  { id: 'docx', x: 520, y: 130, title: 'report.docx', sub: 'output' },
];

const EDGES: [string, string, boolean?][] = [
  ['goal', 'sources'],
  ['goal', 'outline'],
  ['sources', 'draft'],
  ['outline', 'draft'],
  ['draft', 'docx'],
  ['outline', 'cite', true],
  ['cite', 'docx', true],
];

const W = 120;
const H = 40;
const byId = new Map(NODES.map((n) => [n.id, n]));

function curve(a: MiniNode, b: MiniNode): string {
  const sx = a.x + W;
  const sy = a.y + H / 2;
  const tx = b.x;
  const ty = b.y + H / 2;
  const mx = (sx + tx) / 2;
  return `M${sx},${sy} C${mx},${sy} ${mx},${ty} ${tx},${ty}`;
}

/** Decorative, self-running preview of a graph being planned and executed. */
export function HeroGraph() {
  return (
    <svg
      viewBox="0 0 680 340"
      className="hero-graph h-auto w-full"
      role="img"
      aria-label="Animated example: a research report graph whose nodes run in dependency order."
    >
      <defs>
        <marker
          id="hg-arrow"
          viewBox="0 0 10 10"
          refX="9"
          refY="5"
          markerWidth="6"
          markerHeight="6"
          orient="auto"
        >
          <path d="M0,0 L10,5 L0,10 z" fill="var(--canvas-edge)" />
        </marker>
      </defs>
      {EDGES.map(([from, to, ghost], i) => {
        const a = byId.get(from);
        const b = byId.get(to);
        if (!a || !b) return null;
        return (
          <path
            key={`${from}-${to}`}
            d={curve(a, b)}
            className={ghost ? 'hg-edge-ghost' : 'hg-edge'}
            markerEnd={ghost ? undefined : 'url(#hg-arrow)'}
            style={ghost ? undefined : { animationDelay: `${300 + i * 180}ms` }}
          />
        );
      })}
      {NODES.map((node, i) => (
        // Position on the outer group: the CSS pop-in animation owns the inner group's transform.
        <g key={node.id} transform={`translate(${node.x},${node.y})`}>
          <g
            className={node.ghost ? 'hg-node hg-ghost' : 'hg-node'}
            style={{ animationDelay: node.ghost ? '2.4s' : `${i * 140}ms` }}
          >
            <rect className="hg-card" width={W} height={H} rx={10} />
            {!node.ghost && (
              <rect
                className="hg-ring"
                x={-3}
                y={-3}
                width={W + 6}
                height={H + 6}
                rx={13}
                style={{ animationDelay: `${1.2 + (node.x / 160) * 0.9}s` }}
              />
            )}
            <text x={12} y={17}>
              {node.title}
            </text>
            <text className="hg-sub" x={12} y={30}>
              {node.sub}
            </text>
          </g>
        </g>
      ))}
    </svg>
  );
}
