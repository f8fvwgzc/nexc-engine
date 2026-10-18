import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { TooltipProvider } from '@/components/ui/tooltip';
import type { Rollup } from '@/schemas/session';

import { tokensLabel, type TreeRow } from '../tree';
import { SessionTree } from './session-tree';

const rollup = (over: Partial<Rollup> = {}): Rollup => ({
  spans: 3,
  llm_calls: 2,
  tool_calls: 1,
  denials: 0,
  tokens_in: 1200,
  tokens_out: 340,
  tokens_cached: 100,
  cost_usd: 0.0123,
  ...over,
});

const row = (over: Partial<TreeRow> & Pick<TreeRow, 'key' | 'kind' | 'title'>): TreeRow => ({
  depth: 0,
  parentKey: null,
  subtitle: '',
  status: 'running',
  durationMs: 5000,
  rollup: rollup(),
  own: null,
  hasChildren: false,
  spanIds: [],
  cancellable: null,
  agentId: null,
  nodeId: null,
  error: null,
  ...over,
});

const rows: TreeRow[] = [
  row({ key: 'run', kind: 'run', title: 'Run 0190a1b2', subtitle: 'Research', hasChildren: true }),
  row({
    key: 'node:1',
    kind: 'node',
    title: 'Customs',
    subtitle: 'agent',
    depth: 1,
    parentKey: 'run',
    hasChildren: true,
  }),
  row({
    key: 'agent:1:a',
    kind: 'agent',
    title: 'Analyst',
    subtitle: 'agent',
    depth: 2,
    parentKey: 'node:1',
    hasChildren: true,
  }),
  row({
    key: 'agent:1:b',
    kind: 'agent',
    title: 'researcher-1',
    subtitle: 'researcher',
    depth: 3,
    parentKey: 'agent:1:a',
    cancellable: '0190a1b2-c3d4-7e5f-8a9b-2c1d2e3f4a5b',
    rollup: rollup({ denials: 1, cost_usd: 0.002 }),
  }),
];

function renderTree(over: Partial<React.ComponentProps<typeof SessionTree>> = {}) {
  const props = {
    rows,
    expanded: new Set(['run', 'node:1', 'agent:1:a']),
    onToggle: vi.fn(),
    selected: null,
    onSelect: vi.fn(),
    canStop: true,
    runActive: true,
    onStopAgent: vi.fn(),
    onStopRun: vi.fn(),
    ...over,
  };
  render(
    <TooltipProvider>
      <SessionTree {...props} />
    </TooltipProvider>,
  );
  return props;
}

describe('SessionTree', () => {
  it('shows every row with its status, tokens, cost and duration', () => {
    renderTree();
    const items = screen.getAllByRole('treeitem');
    expect(items).toHaveLength(4);
    expect(items[0]).toHaveAttribute('aria-level', '1');
    expect(items[3]).toHaveAttribute('aria-level', '4');
    expect(items[1]).toHaveTextContent('Customs');
    expect(items[3]).toHaveTextContent('researcher-1');
    expect(items[3]).toHaveTextContent('2t · 1c · 1d');
    expect(items[3]).toHaveTextContent('$0.002');
    expect(items[3]).toHaveTextContent('5.0 s');
    expect(screen.getAllByRole('status', { name: 'Running' })).toHaveLength(4);
    expect(tokensLabel(rollup())).toBe('1,200 in · 340 out (100 cached)');
    expect(tokensLabel(rollup({ tokens_cached: 0 }))).toBe('1,200 in · 340 out');
  });

  it('stops a sub-agent and the run behind a confirmation', () => {
    const props = renderTree();
    expect(screen.getAllByRole('button', { name: /^Stop/ })).toHaveLength(2);
    fireEvent.click(screen.getByRole('button', { name: 'Stop researcher-1' }));
    expect(screen.getByRole('alertdialog')).toHaveTextContent('Stop researcher-1?');
    fireEvent.click(screen.getByRole('button', { name: 'Stop the sub-agent' }));
    expect(props.onStopAgent).toHaveBeenCalledWith(rows[3]);
    fireEvent.click(screen.getByRole('button', { name: 'Stop the run' }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel the run' }));
    expect(props.onStopRun).toHaveBeenCalledTimes(1);
  });

  it('offers no stop to those who may not, nor on a finished run', () => {
    renderTree({ canStop: false });
    expect(screen.queryByRole('button', { name: /^Stop/ })).not.toBeInTheDocument();
  });

  it('travels with the keyboard: arrows move, Right opens, Left folds, Enter selects', () => {
    const props = renderTree({ expanded: new Set(['run', 'node:1']) });
    const items = screen.getAllByRole('treeitem');
    items[0]!.focus();
    fireEvent.keyDown(items[0]!, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(items[1]);
    fireEvent.keyDown(items[1]!, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(items[2]);
    // The Analyst row is folded: Right opens it.
    fireEvent.keyDown(items[2]!, { key: 'ArrowRight' });
    expect(props.onToggle).toHaveBeenCalledWith('agent:1:a');
    fireEvent.keyDown(items[2]!, { key: 'Enter' });
    expect(props.onSelect).toHaveBeenCalledWith('agent:1:a');
    // Left on the open node folds it; Left on a leaf goes to its parent.
    fireEvent.keyDown(items[1]!, { key: 'ArrowLeft' });
    expect(props.onToggle).toHaveBeenCalledWith('node:1');
    fireEvent.keyDown(items[3]!, { key: 'ArrowLeft' });
    expect(document.activeElement).toBe(items[2]);
    fireEvent.keyDown(items[2]!, { key: 'End' });
    expect(document.activeElement).toBe(items[3]);
    fireEvent.click(items[3]!);
    expect(props.onSelect).toHaveBeenLastCalledWith('agent:1:b');
  });
});
