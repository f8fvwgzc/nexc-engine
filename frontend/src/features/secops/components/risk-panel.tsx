import { ChartLineIcon } from 'lucide-react';
import { useMemo } from 'react';
import { CartesianGrid, Line, LineChart, XAxis, YAxis } from 'recharts';

import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart';
import { ErrorState } from '@/components/custom-ui/error-state';
import { Skeleton } from '@/components/ui/skeleton';
import type { Agent } from '@/schemas/agent';
import type { SecopsOverview } from '@/schemas/secops';

import { riskSeries, shortDay } from '../risk';
import { Panel, PanelEmpty } from './console-panel';

const COLORS = [
  'var(--chart-1)',
  'var(--chart-2)',
  'var(--chart-3)',
  'var(--chart-4)',
  'var(--chart-5)',
];

/** Denials by rule as bars, the longest first. */
function DenialsByRule({ denials }: { denials: SecopsOverview['denials_fortnight'] }) {
  if (denials.length === 0) {
    return <p className="text-xs text-muted-foreground">Nothing refused in the last fortnight.</p>;
  }
  const most = Math.max(...denials.map((d) => d.count));
  return (
    <ol className="space-y-1" aria-label="Denials by rule, last fourteen days">
      {denials.slice(0, 8).map((d) => (
        <li
          key={d.rule}
          className="grid grid-cols-[minmax(0,10rem)_minmax(0,1fr)_2.5rem] items-center gap-2 text-xs"
        >
          <span className="truncate font-mono" title={d.rule}>
            {d.rule || '(no rule)'}
          </span>
          <span className="h-2 rounded-full bg-muted" aria-hidden>
            <span
              className="block h-2 rounded-full bg-[var(--status-failed)]"
              style={{ width: `${Math.max(4, (d.count / most) * 100)}%` }}
            />
          </span>
          <span className="text-right tabular-nums">{d.count}</span>
        </li>
      ))}
    </ol>
  );
}

interface RiskPanelProps {
  overview: SecopsOverview | undefined;
  /** Why the overview could not be had, while there is none to show. */
  error?: unknown;
  onRetry?: () => void;
  agents: Agent[];
  onOpenAgent: (agentId: string) => void;
  className?: string;
}

/**
 * Risk over the last fourteen days, one line per scored agent, next to the rules that denied
 * most and the agents scoring highest right now.
 */
export function RiskPanel({
  overview,
  error,
  onRetry,
  agents,
  onOpenAgent,
  className,
}: RiskPanelProps) {
  const names = useMemo(() => new Map(agents.map((a) => [a.id, a.name])), [agents]);
  const series = useMemo(
    () => (overview ? riskSeries(overview.risk_history) : { rows: [], agents: [] }),
    [overview],
  );
  const config: ChartConfig = useMemo(
    () =>
      Object.fromEntries(
        series.agents.map((id, i) => [
          id,
          { label: names.get(id) ?? id.slice(0, 8), color: COLORS[i % COLORS.length] },
        ]),
      ),
    [series.agents, names],
  );
  return (
    <Panel id="secops-risk" title="Risk" icon={ChartLineIcon} className={className}>
      {!overview && error ? (
        <ErrorState compact title="Couldn’t load the risk scores" error={error} onRetry={onRetry} />
      ) : !overview ? (
        <Skeleton className="h-40 rounded-lg" />
      ) : (
        <div className="grid gap-4 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
          <div className="min-w-0 space-y-1">
            <p className="text-xs text-muted-foreground">Risk score per agent, last 14 days</p>
            {series.agents.length === 0 ? (
              <PanelEmpty>
                No agent was scored yet: the baseline learns from the first runs.
              </PanelEmpty>
            ) : (
              <ChartContainer
                config={config}
                className="h-40 w-full"
                initialDimension={{ width: 320, height: 160 }}
              >
                <LineChart data={series.rows} margin={{ top: 6, right: 8, bottom: 0, left: -20 }}>
                  <CartesianGrid vertical={false} strokeDasharray="3 3" />
                  <XAxis
                    dataKey="day"
                    tickFormatter={shortDay}
                    tickLine={false}
                    axisLine={false}
                    minTickGap={24}
                    fontSize={10}
                  />
                  <YAxis
                    domain={[0, 10]}
                    ticks={[0, 5, 10]}
                    tickLine={false}
                    axisLine={false}
                    fontSize={10}
                  />
                  <ChartTooltip
                    content={
                      <ChartTooltipContent
                        labelFormatter={(label: unknown) =>
                          typeof label === 'string' ? shortDay(label) : ''
                        }
                      />
                    }
                  />
                  {series.agents.map((id) => (
                    <Line
                      key={id}
                      type="monotone"
                      dataKey={id}
                      stroke={`var(--color-${id})`}
                      strokeWidth={2}
                      dot={{ r: 2 }}
                      connectNulls={false}
                      isAnimationActive={false}
                    />
                  ))}
                </LineChart>
              </ChartContainer>
            )}
          </div>
          <div className="min-w-0 space-y-3">
            <div className="space-y-1">
              <p className="text-xs text-muted-foreground">Highest risk now</p>
              {overview.top_risk.length === 0 ? (
                <p className="text-xs text-muted-foreground">Nobody scored yet.</p>
              ) : (
                <ol className="space-y-0.5" aria-label="Highest-risk agents">
                  {overview.top_risk.map((agent) => (
                    <li key={agent.agent_id} className="flex items-center gap-2 text-xs">
                      <button
                        type="button"
                        className="truncate font-medium hover:underline focus-visible:underline focus-visible:outline-none"
                        onClick={() => onOpenAgent(agent.agent_id)}
                      >
                        {agent.name}
                      </button>
                      <span
                        className={`ml-auto shrink-0 font-medium tabular-nums ${agent.risk_score >= 6 ? 'text-destructive' : ''}`}
                      >
                        {agent.risk_score.toFixed(1)}
                      </span>
                      <span
                        className="hidden max-w-48 truncate text-muted-foreground sm:inline"
                        title={agent.risk_reasons.join('; ')}
                      >
                        {agent.risk_reasons[0] ?? 'nothing unusual'}
                      </span>
                    </li>
                  ))}
                </ol>
              )}
            </div>
            <div className="space-y-1">
              <p className="text-xs text-muted-foreground">Denials by rule, last 14 days</p>
              <DenialsByRule denials={overview.denials_fortnight} />
            </div>
          </div>
        </div>
      )}
    </Panel>
  );
}
