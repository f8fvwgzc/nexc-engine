import { formatTokens } from '@/lib/format';
import type { Rollup, Session, SessionAgent, SessionNode, SessionStatus } from '@/schemas/session';
import type { Span, SpanKind } from '@/schemas/trace';

/** What a row of the outline stands for. */
export type RowKind = 'run' | 'node' | 'agent';

/** One row of the session outline, flattened for rendering and keyboard travel. */
export interface TreeRow {
  /** Stable across refetches: `run`, `node:<id>`, `agent:<node id>:<principal>`. */
  key: string;
  kind: RowKind;
  depth: number;
  parentKey: string | null;
  title: string;
  /** What is shown under the title: a role, an executor, a graph. */
  subtitle: string;
  status: SessionStatus;
  durationMs: number | null;
  /** The subtree's rollup (a node's, an agent's `total`, the run's). */
  rollup: Rollup;
  /** The row's own rollup, when it differs from the subtree's (an agent with sub-agents). */
  own: Rollup | null;
  hasChildren: boolean;
  /** The spans the row opens on the right: an agent's own, a node's context, the run's all. */
  spanIds: string[] | 'all';
  /** A sub-agent an operator may stop now; the spawn span that names it. */
  cancellable: string | null;
  agentId: string | null;
  nodeId: string | null;
  error: string | null;
}

export function runStatusOf(session: Session): SessionStatus {
  return session.status;
}

function agentRows(
  agent: SessionAgent,
  node: SessionNode,
  depth: number,
  parentKey: string,
  expanded: ReadonlySet<string>,
  out: TreeRow[],
): void {
  const key = `agent:${node.node_id}:${agent.principal}`;
  const hasChildren = agent.children.length > 0;
  out.push({
    key,
    kind: 'agent',
    depth,
    parentKey,
    title: displayAgent(agent.principal),
    subtitle: agent.role ?? (agent.depth === 0 ? 'agent' : 'sub-agent'),
    status: agent.status,
    durationMs: agent.duration_ms,
    rollup: agent.total,
    own: hasChildren ? agent.own : null,
    hasChildren,
    spanIds: agent.span_ids,
    cancellable: agent.cancellable ? agent.spawn_span_id : null,
    agentId: agent.agent_id,
    nodeId: node.node_id,
    error: null,
  });
  if (hasChildren && expanded.has(key)) {
    for (const child of agent.children) agentRows(child, node, depth + 1, key, expanded, out);
  }
}

/** The last segment of a principal: `Analyst`, `researcher-1`. */
export function displayAgent(principal: string): string {
  const match = /^agent:\/\/[^/]+\/(.+)$/.exec(principal);
  const path = match?.[1] ?? principal;
  return path.split('/').pop() ?? path;
}

/** Every key with children: what "expand all" opens. */
export function allKeys(session: Session): Set<string> {
  const keys = new Set<string>(['run']);
  const walk = (agent: SessionAgent, nodeId: string) => {
    keys.add(`agent:${nodeId}:${agent.principal}`);
    for (const child of agent.children) walk(child, nodeId);
  };
  for (const node of session.nodes) {
    keys.add(`node:${node.node_id}`);
    for (const agent of node.agents) walk(agent, node.node_id);
  }
  return keys;
}

/**
 * The outline as rows, top to bottom: the run, its nodes, the agent each ran as and its
 * sub-agents, showing only what is under an expanded row.
 */
export function flattenSession(session: Session, expanded: ReadonlySet<string>): TreeRow[] {
  const rows: TreeRow[] = [
    {
      key: 'run',
      kind: 'run',
      depth: 0,
      parentKey: null,
      title: `Run ${session.run_id.slice(0, 8)}`,
      subtitle: session.graph_name,
      status: session.status,
      durationMs: session.duration_ms,
      rollup: session.rollup,
      own: null,
      hasChildren: session.nodes.length > 0,
      spanIds: 'all',
      cancellable: null,
      agentId: null,
      nodeId: null,
      error: null,
    },
  ];
  if (!expanded.has('run')) return rows;
  for (const node of session.nodes) {
    const key = `node:${node.node_id}`;
    const hasChildren = node.agents.length > 0;
    rows.push({
      key,
      kind: 'node',
      depth: 1,
      parentKey: 'run',
      title: node.title || node.node_id.slice(0, 8),
      subtitle: node.cached ? 'served from cache' : node.executor,
      status: node.status,
      durationMs: node.duration_ms,
      rollup: node.rollup,
      own: null,
      hasChildren,
      spanIds: node.context_span_ids,
      cancellable: null,
      agentId: node.agents[0]?.agent_id ?? null,
      nodeId: node.node_id,
      error: node.error,
    });
    if (hasChildren && expanded.has(key)) {
      for (const agent of node.agents) agentRows(agent, node, 2, key, expanded, rows);
    }
  }
  return rows;
}

/** What the outline is narrowed to. */
export interface TreeFilter {
  /** A workspace agent's id; `all` for every agent. */
  agent: string;
  /** A span kind the subtree must contain; `all` for any. */
  kind: 'all' | SpanKind;
  status: 'all' | SessionStatus;
}

/**
 * The rows that pass the filter, with their ancestors so the outline stays a tree: a row
 * passes when it matches itself or a row under it does.
 */
export function filterRows(rows: TreeRow[], filter: TreeFilter, spansById: Map<string, Span>) {
  const matches = (row: TreeRow): boolean => {
    if (filter.agent !== 'all' && row.kind === 'agent' && row.agentId !== filter.agent)
      return false;
    if (filter.status !== 'all' && row.status !== filter.status) return false;
    if (filter.kind !== 'all') {
      if (row.spanIds === 'all') return true;
      if (!row.spanIds.some((id) => spansById.get(id)?.kind === filter.kind)) return false;
    }
    return true;
  };
  if (filter.agent === 'all' && filter.status === 'all' && filter.kind === 'all') return rows;
  const keep = new Set<string>();
  const byKey = new Map(rows.map((r) => [r.key, r]));
  for (const row of rows) {
    if (row.kind !== 'run' && !matches(row)) continue;
    let current: TreeRow | undefined = row;
    while (current) {
      keep.add(current.key);
      current = current.parentKey ? byKey.get(current.parentKey) : undefined;
    }
  }
  return rows.filter((r) => keep.has(r.key));
}

/** The spans of a row, in chain order. */
export function spansOfRow(row: TreeRow, spans: Span[]): Span[] {
  if (row.spanIds === 'all') return spans;
  const wanted = new Set(row.spanIds);
  return spans.filter((s) => wanted.has(s.id));
}

/** Tokens in and out of a rollup, for a label. */
export function tokensOf(rollup: Rollup): number {
  return rollup.tokens_in + rollup.tokens_out;
}

/** Tokens in, out and cached of a rollup, as one short line. */
export function tokensLabel(rollup: Rollup): string {
  const cached = rollup.tokens_cached > 0 ? ` (${formatTokens(rollup.tokens_cached)} cached)` : '';
  return `${formatTokens(rollup.tokens_in)} in · ${formatTokens(rollup.tokens_out)} out${cached}`;
}
