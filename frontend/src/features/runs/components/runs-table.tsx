import { Link } from 'react-router-dom';

import { StatusBadge } from '@/components/custom-ui/status-badge';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import {
  durationBetween,
  formatCost,
  formatDuration,
  formatRelative,
  formatTokens,
} from '@/lib/format';
import type { Run } from '@/schemas/run';

export interface RunRow {
  run: Run;
  graphName: string;
}

const TERMINAL = new Set(['succeeded', 'failed', 'skipped', 'cancelled']);

export function RunsTable({ rows }: { rows: RunRow[] }) {
  return (
    <div className="overflow-x-auto rounded-xl border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Run</TableHead>
            <TableHead>Graph</TableHead>
            <TableHead>Status</TableHead>
            <TableHead className="hidden text-right sm:table-cell">Nodes</TableHead>
            <TableHead className="hidden text-right sm:table-cell">Tokens</TableHead>
            <TableHead className="hidden text-right sm:table-cell">Cost</TableHead>
            <TableHead className="hidden sm:table-cell">Started</TableHead>
            <TableHead className="hidden text-right md:table-cell">Duration</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map(({ run, graphName }) => {
            const done = run.node_runs.filter((nr) => TERMINAL.has(nr.status)).length;
            const duration = durationBetween(run.started_at, run.finished_at);
            return (
              <TableRow key={run.id} className="group">
                <TableCell>
                  <Link
                    to={`/app/runs/${run.id}`}
                    className="font-mono text-xs font-medium underline-offset-4 group-hover:underline"
                  >
                    {run.id.slice(0, 8)}
                  </Link>
                </TableCell>
                <TableCell className="max-w-36 truncate sm:max-w-48" title={graphName}>
                  <Link to={`/app/graphs/${run.graph_id}`} className="hover:underline">
                    {graphName}
                  </Link>
                  {/* On a phone the start time goes under the graph, its column being gone. */}
                  <span className="block text-xs text-muted-foreground sm:hidden">
                    {formatRelative(run.started_at ?? run.created_at)}
                  </span>
                </TableCell>
                <TableCell>
                  <StatusBadge status={run.status} />
                </TableCell>
                <TableCell className="hidden text-right tabular-nums sm:table-cell">
                  {done}/{run.node_runs.length}
                </TableCell>
                <TableCell className="hidden text-right tabular-nums sm:table-cell">
                  {formatTokens(run.tokens_in + run.tokens_out)}
                </TableCell>
                <TableCell className="hidden text-right tabular-nums sm:table-cell">
                  {formatCost(run.cost_usd)}
                </TableCell>
                <TableCell className="hidden whitespace-nowrap text-muted-foreground sm:table-cell">
                  {formatRelative(run.started_at ?? run.created_at)}
                </TableCell>
                <TableCell className="hidden text-right tabular-nums md:table-cell">
                  {duration === null ? '—' : formatDuration(duration)}
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </div>
  );
}
