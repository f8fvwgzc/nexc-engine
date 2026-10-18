import {
  BotIcon,
  ChevronRightIcon,
  GitForkIcon,
  ListTodoIcon,
  type LucideIcon,
  PlayIcon,
  SquareIcon,
} from 'lucide-react';
import { useRef, useState, type KeyboardEvent } from 'react';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { STATUS_META } from '@/components/custom-ui/status-meta';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { formatCost, formatDuration } from '@/lib/format';
import { cn } from '@/lib/utils';

import { tokensLabel, type TreeRow } from '../tree';

const ICONS: Record<TreeRow['kind'], LucideIcon> = {
  run: PlayIcon,
  node: ListTodoIcon,
  agent: BotIcon,
};

function StatusDot({ status }: { status: TreeRow['status'] }) {
  const meta = STATUS_META[status];
  const Icon = meta.icon;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          role="status"
          aria-label={meta.label}
          style={{ '--dot': meta.color } as React.CSSProperties}
          className={cn(
            'inline-flex size-5 shrink-0 items-center justify-center rounded-full bg-[color-mix(in_oklch,var(--dot)_15%,transparent)] text-[var(--dot)]',
            status === 'running' && 'motion-safe:animate-pulse-ring',
          )}
        >
          <Icon className={cn('size-3', status === 'running' && 'motion-safe:animate-spin')} />
        </span>
      </TooltipTrigger>
      <TooltipContent>{meta.label}</TooltipContent>
    </Tooltip>
  );
}

interface StopTarget {
  row: TreeRow;
}

export interface SessionTreeProps {
  rows: TreeRow[];
  expanded: ReadonlySet<string>;
  onToggle: (key: string) => void;
  selected: string | null;
  onSelect: (key: string) => void;
  /** Whether the viewer may stop agents and the run. */
  canStop: boolean;
  /** The run is still going: its row offers the stop. */
  runActive: boolean;
  onStopAgent: (row: TreeRow) => void;
  onStopRun: () => void;
  className?: string;
}

/**
 * The session as an outline: the run, its nodes, the agent each ran as and its sub-agents,
 * each row with its kind, status, duration, tokens and cost, and a Stop on what can still be
 * stopped. Arrow keys travel the rows, Right and Left open and fold them, Enter selects.
 */
export function SessionTree({
  rows,
  expanded,
  onToggle,
  selected,
  onSelect,
  canStop,
  runActive,
  onStopAgent,
  onStopRun,
  className,
}: SessionTreeProps) {
  const [focusKey, setFocusKey] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<StopTarget | null>(null);
  const [confirmRun, setConfirmRun] = useState(false);
  const refs = useRef(new Map<string, HTMLDivElement>());
  // A focus key the rows no longer hold (folded away, filtered out) falls back to the first row.
  const active = focusKey && rows.some((r) => r.key === focusKey) ? focusKey : rows[0]?.key;

  const focusRow = (key: string) => {
    setFocusKey(key);
    refs.current.get(key)?.focus();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>, row: TreeRow, index: number) => {
    const move = (to: number) => {
      const target = rows[Math.max(0, Math.min(rows.length - 1, to))];
      if (target) focusRow(target.key);
    };
    switch (event.key) {
      case 'ArrowDown':
        move(index + 1);
        break;
      case 'ArrowUp':
        move(index - 1);
        break;
      case 'Home':
        move(0);
        break;
      case 'End':
        move(rows.length - 1);
        break;
      case 'ArrowRight':
        if (row.hasChildren && !expanded.has(row.key)) onToggle(row.key);
        else if (row.hasChildren) move(index + 1);
        break;
      case 'ArrowLeft':
        if (row.hasChildren && expanded.has(row.key)) onToggle(row.key);
        else if (row.parentKey) focusRow(row.parentKey);
        break;
      case 'Enter':
      case ' ':
        onSelect(row.key);
        break;
      default:
        return;
    }
    event.preventDefault();
  };

  return (
    <div
      role="tree"
      aria-label="Session"
      className={cn('divide-y rounded-xl border bg-card', className)}
    >
      {rows.map((row, index) => {
        const Icon = row.kind === 'agent' && row.depth > 2 ? GitForkIcon : ICONS[row.kind];
        const open = expanded.has(row.key);
        const isSelected = selected === row.key;
        const stoppable =
          canStop && (row.cancellable !== null || (row.kind === 'run' && runActive));
        return (
          <div
            key={row.key}
            ref={(el) => {
              if (el) refs.current.set(row.key, el);
              else refs.current.delete(row.key);
            }}
            role="treeitem"
            aria-level={row.depth + 1}
            aria-expanded={row.hasChildren ? open : undefined}
            aria-selected={isSelected}
            tabIndex={active === row.key ? 0 : -1}
            data-key={row.key}
            onFocus={() => setFocusKey(row.key)}
            onKeyDown={(event) => onKeyDown(event, row, index)}
            onClick={() => onSelect(row.key)}
            className={cn(
              'group grid cursor-pointer grid-cols-[minmax(0,1fr)_auto] items-center gap-2 px-2 py-1.5 text-sm outline-none focus-visible:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset',
              isSelected && 'bg-brand/10',
              !isSelected && 'hover:bg-muted/40',
            )}
          >
            <div
              className="flex min-w-0 items-center gap-1.5"
              style={{ paddingLeft: `${Math.min(row.depth, 6) * 0.9}rem` }}
            >
              {row.hasChildren ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  aria-label={open ? `Fold ${row.title}` : `Unfold ${row.title}`}
                  onClick={(event) => {
                    event.stopPropagation();
                    onToggle(row.key);
                  }}
                >
                  <ChevronRightIcon className={cn('transition-transform', open && 'rotate-90')} />
                </Button>
              ) : (
                <span className="size-6 shrink-0" aria-hidden />
              )}
              <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden />
              <StatusDot status={row.status} />
              <span className="min-w-0">
                <span className="block truncate font-medium" title={row.title}>
                  {row.title}
                </span>
                <span className="block truncate text-[11px] text-muted-foreground">
                  {row.subtitle}
                  {row.error ? ` · ${row.error}` : ''}
                </span>
              </span>
            </div>
            <div className="flex shrink-0 items-center gap-2 text-xs text-muted-foreground tabular-nums">
              <span className="hidden lg:inline" title="Tokens in, out and cached">
                {tokensLabel(row.rollup)}
              </span>
              <span className="hidden sm:inline" title="Model turns · tool calls · denials">
                {row.rollup.llm_calls}t · {row.rollup.tool_calls}c
                {row.rollup.denials > 0 ? ` · ${row.rollup.denials}d` : ''}
              </span>
              <span className="w-14 text-right" title="Cost">
                {formatCost(row.rollup.cost_usd)}
              </span>
              <span className="w-14 text-right" title="Duration">
                {row.durationMs === null ? '—' : formatDuration(row.durationMs)}
              </span>
              {stoppable && (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      variant="destructive"
                      size="xs"
                      aria-label={row.kind === 'run' ? 'Stop the run' : `Stop ${row.title}`}
                      onClick={(event) => {
                        event.stopPropagation();
                        if (row.kind === 'run') setConfirmRun(true);
                        else setConfirm({ row });
                      }}
                    >
                      <SquareIcon className="fill-current" />
                      <span className="hidden sm:inline">Stop</span>
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>
                    {row.kind === 'run'
                      ? 'Cancel the whole run'
                      : `Stop ${row.title}; the run goes on`}
                  </TooltipContent>
                </Tooltip>
              )}
            </div>
          </div>
        );
      })}
      <ConfirmDialog
        open={confirm !== null}
        onOpenChange={(open) => !open && setConfirm(null)}
        title={confirm ? `Stop ${confirm.row.title}?` : 'Stop the sub-agent?'}
        description="Its task is cancelled and its parent sees the tool error “cancelled by an operator”; the rest of the run goes on. The stop is recorded on the trace and in the audit log."
        confirmLabel="Stop the sub-agent"
        destructive
        onConfirm={() => {
          if (confirm) onStopAgent(confirm.row);
          setConfirm(null);
        }}
      />
      <ConfirmDialog
        open={confirmRun}
        onOpenChange={setConfirmRun}
        title="Cancel the whole run?"
        description="Every node still running is cancelled; what finished stays."
        confirmLabel="Cancel the run"
        destructive
        onConfirm={() => {
          onStopRun();
          setConfirmRun(false);
        }}
      />
    </div>
  );
}
