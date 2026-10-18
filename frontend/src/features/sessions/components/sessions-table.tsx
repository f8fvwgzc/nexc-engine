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
            <TableHead className="hidden md:table-cell">Agents</TableHead>
            <TableHead className="hidden text-right lg:table-cell">Nodes</TableHead>
            <TableHead className="hidden text-right lg:table-cell">Turns · calls</TableHead>
            <TableHead className="hidden text-right sm:table-cell">Tokens</TableHead>
            <TableHead className="hidden text-right sm:table-cell">Cost</TableHead>
            <TableHead className="hidden text-right md:table-cell">Duration</TableHead>
            <TableHead className="hidden sm:table-cell">Started</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((s) => (
            <TableRow key={s.run_id} className="group">
              <TableCell className="max-w-56">
                <Link
                  to={`/app/sessions/${s.run_id}`}
                  className="block truncate font-medium underline-offset-4 group-hover:underline"
                  title={`${s.run_id.slice(0, 8)} · ${s.graph_name}`}
                >
                  <span className="font-mono text-xs">{s.run_id.slice(0, 8)}</span>
                  <span className="ml-2 text-muted-foreground">{s.graph_name}</span>
                </Link>
              </TableCell>
              <TableCell>
                <StatusBadge status={s.status} />
              </TableCell>
              <TableCell
                className="hidden max-w-48 truncate md:table-cell"
                title={s.agents.map((a) => a.name).join(', ')}
              >
                {s.agents.length === 0 ? (
                  <span className="text-muted-foreground">—</span>
                ) : (
                  s.agents.map((a) => a.name).join(', ')
                )}
              </TableCell>
              <TableCell className="hidden text-right tabular-nums lg:table-cell">
                {s.nodes}
              </TableCell>
              <TableCell className="hidden text-right tabular-nums lg:table-cell">
                {s.rollup.llm_calls} · {s.rollup.tool_calls}
                {s.rollup.denials > 0 && (
                  <span className="text-destructive"> · {s.rollup.denials} denied</span>
                )}
              </TableCell>
              <TableCell
                className="hidden text-right tabular-nums sm:table-cell"
                title={`${formatTokens(s.rollup.tokens_in)} in · ${formatTokens(s.rollup.tokens_out)} out · ${formatTokens(s.rollup.tokens_cached)} cached`}
              >
                {formatTokens(s.rollup.tokens_in + s.rollup.tokens_out)}
              </TableCell>
              <TableCell className="hidden text-right tabular-nums sm:table-cell">
                {formatCost(s.rollup.cost_usd)}
              </TableCell>
              <TableCell className="hidden text-right tabular-nums md:table-cell">
                {s.duration_ms === null ? '—' : formatDuration(s.duration_ms)}
              </TableCell>
              <TableCell className="hidden whitespace-nowrap text-muted-foreground sm:table-cell">
                {formatRelative(s.started_at ?? s.created_at)}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
