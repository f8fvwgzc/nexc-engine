import type { Span } from '@/schemas/trace';

/** A span and what hangs under it: a turn's tool calls and spawns. */
export interface TraceRow {
  span: Span;
  children: TraceRow[];
}

/** One acting agent within a node: the root agent, a sub-agent, or the system. */
export interface AgentGroup {
  principal: string;
  rows: TraceRow[];
}

/** Everything one node did in the run. */
export interface NodeGroup {
  nodeId: string;
  /** What the node was given to read (`data_read`), before it ran. */
  context: Span[];
  agents: AgentGroup[];
  spans: number;
  errors: number;
  start: number;
  end: number;
}

/** The time window the waterfall's bars are drawn in. */
export interface TraceWindow {
  start: number;
  end: number;
}

const startOf = (s: Span) => Date.parse(s.started_at);
const endOf = (s: Span) => (s.ended_at ? Date.parse(s.ended_at) : Date.parse(s.started_at));
const byStart = (a: Span, b: Span) => startOf(a) - startOf(b) || a.seq - b.seq;

export function traceWindow(spans: Span[]): TraceWindow {
  if (spans.length === 0) return { start: 0, end: 0 };
  return {
    start: Math.min(...spans.map(startOf)),
    end: Math.max(...spans.map(endOf)),
  };
}

/** Left offset and width of a span's bar in the window, in percent (at least a sliver). */
export function barOf(span: Span, window: TraceWindow): { left: number; width: number } {
  const total = Math.max(1, window.end - window.start);
  const left = Math.min(99.5, Math.max(0, ((startOf(span) - window.start) / total) * 100));
  const width = Math.max(0.5, ((endOf(span) - startOf(span)) / total) * 100);
  return { left, width: Math.min(100 - left, width) };
}

export function durationOf(span: Span): number | null {
  return span.ended_at ? endOf(span) - startOf(span) : null;
}

/**
 * Groups a run's spans node → agent → turn → tool call. A span is placed under a turn when its
 * parent is an `llm_call` of the same agent (tool calls, spawns); every other span (turns, the
 * built-in executor's call, spans whose parent is unknown) is a row of its agent. A sub-agent is
 * its own group, next to the agent that spawned it. Nodes come in the order they started.
 */
export function buildTraceTree(spans: Span[]): NodeGroup[] {
  const byId = new Map(spans.map((s) => [s.id, s]));
  const nodes = new Map<string, Span[]>();
  for (const span of spans) {
    const list = nodes.get(span.node_id) ?? [];
    list.push(span);
    nodes.set(span.node_id, list);
  }
  const groups: NodeGroup[] = [];
  for (const [nodeId, list] of nodes) {
    list.sort(byStart);
    const context = list.filter((s) => s.kind === 'data_read');
    const rows = new Map<string, TraceRow>();
    const agents = new Map<string, AgentGroup>();
    for (const span of list) {
      if (span.kind === 'data_read') continue;
      const row: TraceRow = { span, children: [] };
      rows.set(span.id, row);
      const parent = span.parent_id ? byId.get(span.parent_id) : undefined;
      const parentRow = parent ? rows.get(parent.id) : undefined;
      const underTurn =
        parent?.kind === 'llm_call' && parent.principal === span.principal && parentRow;
      if (underTurn) {
        parentRow.children.push(row);
        continue;
      }
      const group = agents.get(span.principal) ?? { principal: span.principal, rows: [] };
      group.rows.push(row);
      agents.set(span.principal, group);
    }
    // A child that started before its parent was received still finds it.
    for (const span of list) {
      const row = rows.get(span.id);
      const parent = span.parent_id ? byId.get(span.parent_id) : undefined;
      const parentRow = parent ? rows.get(parent.id) : undefined;
      if (!row || !parent || !parentRow || parent.kind !== 'llm_call') continue;
      if (parent.principal !== span.principal || parentRow.children.includes(row)) continue;
      const group = agents.get(span.principal);
      if (!group) continue;
      group.rows = group.rows.filter((r) => r !== row);
      parentRow.children.push(row);
      parentRow.children.sort((a, b) => byStart(a.span, b.span));
    }
    const window = traceWindow(list);
    groups.push({
      nodeId,
      context,
      agents: [...agents.values()].filter((g) => g.rows.length > 0),
      spans: list.length,
      errors: list.filter((s) => s.status === 'error').length,
      start: window.start,
      end: window.end,
    });
  }
  return groups.sort((a, b) => a.start - b.start);
}

/** A short description of a span's attributes, for its row. */
export function describeSpan(span: Span): string {
  const a = span.attributes;
  const num = (key: string) => (typeof a[key] === 'number' ? a[key] : null);
  switch (span.kind) {
    case 'llm_call': {
      const parts: string[] = [];
      const turn = num('turn');
      if (turn !== null) parts.push(`turn ${turn}`);
      const tin = num('tokens_in');
      const tout = num('tokens_out');
      if (tin !== null && tout !== null) parts.push(`${tin} in / ${tout} out`);
      const evidence = typeof a.evidence === 'string' ? a.evidence : '';
      const coverage = num('evidence_coverage');
      if (evidence) parts.push(`evidence ${evidence}${coverage === null ? '' : ` ${coverage}%`}`);
      return parts.join(' · ');
    }
    case 'tool_call': {
      const bytes = num('result_bytes');
      return bytes === null ? '' : `${bytes} B result`;
    }
    case 'spawn': {
      const role = typeof a.role === 'string' ? a.role : '';
      const budget = num('budget_tokens');
      return [role, budget === null ? '' : `${budget} tokens`].filter(Boolean).join(' · ');
    }
    case 'data_read': {
      const count = (key: string) => (Array.isArray(a[key]) ? (a[key] as unknown[]).length : 0);
      return `${count('document_chunk_ids')} passages · ${count('memory_ids')} memories · ${count('upstream_node_ids')} upstream`;
    }
    case 'policy': {
      const text = (key: string) => (typeof a[key] === 'string' ? a[key] : '');
      const withheld = Array.isArray(a.withheld_document_chunk_ids)
        ? (a.withheld_document_chunk_ids as unknown[]).length +
          (Array.isArray(a.withheld_memory_ids) ? (a.withheld_memory_ids as unknown[]).length : 0)
        : 0;
      const what = withheld > 0 ? `${withheld} withheld` : text('reason');
      const decision =
        text('decision') === 'require_approval' ? 'waits for approval' : text('decision');
      return [decision, text('rule'), what].filter(Boolean).join(' · ');
    }
    case 'approval': {
      const text = (key: string) => (typeof a[key] === 'string' ? a[key] : '');
      const who = text('decided_by_name');
      const waited = num('waited_ms');
      return [
        who ? `${text('decision')} by ${who}` : text('decision'),
        text('reason'),
        waited === null ? '' : `after ${Math.round(waited / 1000)}s`,
      ]
        .filter(Boolean)
        .join(' · ');
    }
    default:
      return '';
  }
}
