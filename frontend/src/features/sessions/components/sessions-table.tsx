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
import { formatCost, formatDuration, formatRelative, formatTokens } from '@/lib/format';
import type { SessionSummary } from '@/schemas/session';

/** The workspace's recent runs with their rollups; a row opens the session. */
export function SessionsTable({ rows }: { rows: SessionSummary[] }) {
  return (
    <div className="overflow-x-auto rounded-xl border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Run</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Agents</TableHead>
            <TableHead className="text-right">Nodes</TableHead>
            <TableHead className="text-right">Turns · calls</TableHead>
            <TableHead className="text-right">Tokens</TableHead>
            <TableHead className="text-right">Cost</TableHead>
            <TableHead className="text-right">Duration</TableHead>
            <TableHead>Started</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((s) => (
            <TableRow key={s.run_id} className="group">
              <TableCell className="max-w-56">
                <Link
                  to={`/app/sessions/${s.run_id}`}
                  className="block truncate font-medium underline-offset-4 group-hover:underline"
                >
                  <span className="font-mono text-xs">{s.run_id.slice(0, 8)}</span>
                  <span className="ml-2 text-muted-foreground">{s.graph_name}</span>
                </Link>
              </TableCell>
              <TableCell>
                <StatusBadge status={s.status} />
              </TableCell>
              <TableCell className="max-w-48 truncate">
                {s.agents.length === 0 ? (
                  <span className="text-muted-foreground">—</span>
                ) : (
                  s.agents.map((a) => a.name).join(', ')
                )}
              </TableCell>
              <TableCell className="text-right tabular-nums">{s.nodes}</TableCell>
              <TableCell className="text-right tabular-nums">
                {s.rollup.llm_calls} · {s.rollup.tool_calls}
                {s.rollup.denials > 0 && (
                  <span className="text-destructive"> · {s.rollup.denials} denied</span>
                )}
              </TableCell>
              <TableCell
                className="text-right tabular-nums"
                title={`${formatTokens(s.rollup.tokens_in)} in · ${formatTokens(s.rollup.tokens_out)} out · ${formatTokens(s.rollup.tokens_cached)} cached`}
              >
                {formatTokens(s.rollup.tokens_in + s.rollup.tokens_out)}
              </TableCell>
              <TableCell className="text-right tabular-nums">
                {formatCost(s.rollup.cost_usd)}
              </TableCell>
              <TableCell className="text-right tabular-nums">
                {s.duration_ms === null ? '—' : formatDuration(s.duration_ms)}
              </TableCell>
              <TableCell className="text-muted-foreground">
                {formatRelative(s.started_at ?? s.created_at)}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
