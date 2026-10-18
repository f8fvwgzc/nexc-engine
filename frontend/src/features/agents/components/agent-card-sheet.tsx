import { useQuery } from '@tanstack/react-query';
import { PencilIcon, ShieldBanIcon, ShieldCheckIcon } from 'lucide-react';
import { Link } from 'react-router-dom';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { CopyButton } from '@/components/custom-ui/copy-button';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { AskAuditPanel } from '@/features/audit/components/ask-audit-panel';
import { alertsQuery, approvalsQuery } from '@/features/policies/api';
import { ResolvedPolicyView } from '@/features/policies/components/resolved-policy';
import { SPAN_KIND_META } from '@/features/runs/span-kinds';
import { orderAlerts } from '@/features/secops/alerts';
import { AlertRow } from '@/features/secops/components/alerts-panel';
import { ApprovalRow } from '@/features/secops/components/approvals-panel';
import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { formatDateTime, formatRelative, formatTokens } from '@/lib/format';
import { cn } from '@/lib/utils';
import { displayPrincipal, type Agent, type AgentCard } from '@/schemas/agent';
import type { Span } from '@/schemas/trace';

import { agentCardQuery } from '../api';
import { summarizeCapabilities } from '../capabilities';
import { useQuarantineAgent } from '../hooks/use-agent-mutations';
import { AgentA2a } from './agent-a2a';
import { AgentStatusBadge } from './agent-status-badge';
import { AgentTokens } from './agent-tokens';
import { BudgetProgress } from './budget-progress';

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="truncate text-sm">{children}</dd>
    </div>
  );
}

/** What a span did, in a few words: the rule for a policy decision, the name otherwise. */
function spanSummary(span: Span): string {
  const a = span.attributes;
  if (span.kind === 'policy') {
    const rule = typeof a.rule === 'string' ? a.rule : '';
    const decision = typeof a.decision === 'string' ? a.decision : 'deny';
    return `${decision} ${span.name}${rule ? ` · ${rule}` : ''}`;
  }
  return span.name;
}

function RecentSpans({ card }: { card: AgentCard }) {
  if (card.recent_spans.length === 0) {
    return <p className="text-xs text-muted-foreground">No actions on record yet.</p>;
  }
  return (
    <ol className="divide-y rounded-lg border" aria-label="Last actions">
      {card.recent_spans.map((span) => {
        const meta = SPAN_KIND_META[span.kind];
        const Icon = meta.icon;
        const error = span.status === 'error';
        const actor = displayPrincipal(span.principal);
        const own = displayPrincipal(card.agent.principal_id);
        return (
          <li key={span.id}>
            <Link
              to={`/app/runs/${span.run_id}?span=${span.id}`}
              className="flex items-center gap-2 px-3 py-1.5 text-xs outline-none hover:bg-muted/60 focus-visible:bg-muted/60"
            >
              <Icon
                className={cn(
                  'size-3.5 shrink-0 text-muted-foreground',
                  error && 'text-destructive',
                )}
                aria-label={meta.label}
              />
              <span
                className={cn('min-w-0 flex-1 truncate font-mono', error && 'text-destructive')}
              >
                {spanSummary(span)}
              </span>
              {actor !== own && (
                <span className="hidden shrink-0 text-muted-foreground sm:inline">
                  {actor.slice(own.length + 1)}
                </span>
              )}
              <time
                dateTime={span.started_at}
                className="shrink-0 text-muted-foreground tabular-nums"
              >
                {formatRelative(span.started_at)}
              </time>
            </Link>
          </li>
        );
      })}
    </ol>
  );
}

function Evaluation({ card }: { card: AgentCard }) {
  const evaluation = card.evaluation;
  if (!evaluation) return null;
  const good = evaluation.total > 0 && evaluation.score * 2 >= evaluation.total;
  return (
    <section className="space-y-1.5">
      <h3 className="text-xs font-medium text-muted-foreground">Evaluation</h3>
      <p className="text-sm">
        <span className={cn('font-medium tabular-nums', !good && 'text-destructive')}>
          {evaluation.score} / {evaluation.total}
        </span>
        <span className="text-muted-foreground">
          {' '}
          competency questions · {evaluation.model} · {formatRelative(evaluation.evaluated_at)}
        </span>
      </p>
      <ul className="space-y-1 text-xs">
        {evaluation.questions.map((q) => (
          <li key={q.question} className="flex gap-2">
            <span
              className={cn(
                'shrink-0 font-medium',
                q.passed ? 'text-[var(--status-succeeded)]' : 'text-destructive',
              )}
              aria-label={q.passed ? 'passed' : 'failed'}
            >
              {q.passed ? '✓' : '✗'}
            </span>
            <span className="min-w-0">
              <span>{q.question}</span>
              {q.answer && (
                <span className="block truncate text-muted-foreground" title={q.answer}>
                  {q.answer}
                </span>
              )}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function Overview({
  shown,
  card,
  isPending,
  admin,
}: {
  shown: Agent;
  card: AgentCard | undefined;
  isPending: boolean;
  admin: boolean;
}) {
  return (
    <div className="space-y-5">
      <p className="text-sm">
        {shown.purpose || <span className="text-muted-foreground">No purpose written yet.</span>}
      </p>
      <dl className="grid grid-cols-2 gap-x-4 gap-y-3">
        <Fact label="Owner">
          {isPending ? <Skeleton className="h-4 w-24" /> : (card?.owner?.name ?? '—')}
        </Fact>
        <Fact label="Team">
          {isPending ? (
            <Skeleton className="h-4 w-16" />
          ) : card?.team ? (
            `${card.team.name} (${card.team.key})`
          ) : (
            '—'
          )}
        </Fact>
        <Fact label="Model">
          {shown.model || 'default'} · {shown.runtime}
        </Fact>
        <Fact label="Role">
          <code className="rounded bg-muted px-1 py-0.5 text-xs">{shown.role}</code>
        </Fact>
        <Fact label="Budget left">
          {card?.budget_left === null
            ? 'no limit'
            : card
              ? `${formatTokens(card.budget_left)} tokens`
              : '…'}
        </Fact>
        <Fact label="Last heartbeat">
          {shown.heartbeat_at ? formatRelative(shown.heartbeat_at) : 'never'}
        </Fact>
        <Fact label="Policy version">
          <Badge variant="outline">v{shown.policy_version}</Badge>
        </Fact>
        <Fact label="Created">{formatDateTime(shown.created_at)}</Fact>
      </dl>
      <BudgetProgress spent={shown.spent_tokens} budget={shown.budget_tokens} />
      <section className="space-y-1.5">
        <h3 className="text-xs font-medium text-muted-foreground">Risk</h3>
        {shown.risk_updated_at === null ? (
          <p className="text-xs text-muted-foreground">
            Not scored yet: the baseline learns from the first three runs.
          </p>
        ) : (
          <div className="space-y-1">
            <p className="text-sm">
              <span
                className={cn(
                  'font-medium tabular-nums',
                  shown.risk_score >= 6 && 'text-destructive',
                )}
              >
                {shown.risk_score.toFixed(1)}
              </span>
              <span className="text-muted-foreground"> / 10 · its latest run</span>
            </p>
            {shown.risk_reasons.length === 0 ? (
              <p className="text-xs text-muted-foreground">Nothing unusual.</p>
            ) : (
              <ul className="list-disc space-y-0.5 pl-4 text-xs">
                {shown.risk_reasons.map((reason) => (
                  <li key={reason}>{reason}</li>
                ))}
              </ul>
            )}
          </div>
        )}
      </section>
      <section className="space-y-1.5">
        <h3 className="text-xs font-medium text-muted-foreground">Scopes</h3>
        <ul className="space-y-0.5 text-xs">
          {summarizeCapabilities(shown.capabilities).map((line) => (
            <li key={line} className="truncate">
              {line}
            </li>
          ))}
        </ul>
      </section>
      {shown.runtime === 'external' && <AgentTokens agentId={shown.id} admin={admin} />}
      {card && <Evaluation card={card} />}
    </div>
  );
}

/** The agent's held calls (admins see them; others are told where they are). */
function AgentApprovals({
  workspaceId,
  agentId,
  admin,
}: {
  workspaceId: string;
  agentId: string;
  admin: boolean;
}) {
  const { data, isPending, error } = useQuery({ ...approvalsQuery(workspaceId), enabled: admin });
  if (!admin) {
    return (
      <p className="text-xs text-muted-foreground">
        Admins see an agent's approvals; yours reach you in the Inbox.
      </p>
    );
  }
  if (isPending) return <Skeleton className="h-16 rounded-lg" />;
  if (error) {
    return (
      <p role="alert" className="text-xs text-destructive">
        {errorMessage(error)}
      </p>
    );
  }
  const own = data.filter((a) => a.agent_id === agentId);
  if (own.length === 0) {
    return (
      <p className="text-xs text-muted-foreground">No tool call of this agent was ever held.</p>
    );
  }
  return (
    <ul className="divide-y rounded-lg border" aria-label="Approvals">
      {own.map((approval) => (
        <ApprovalRow key={approval.id} approval={approval} workspaceId={workspaceId} decide />
      ))}
    </ul>
  );
}

/** The alerts about the agent, unacknowledged first. */
function AgentAlerts({
  workspaceId,
  agentId,
  admin,
}: {
  workspaceId: string;
  agentId: string;
  admin: boolean;
}) {
  const { data, isPending, error } = useQuery({ ...alertsQuery(workspaceId), enabled: admin });
  if (!admin) return <p className="text-xs text-muted-foreground">Admins see alerts.</p>;
  if (isPending) return <Skeleton className="h-16 rounded-lg" />;
  if (error) {
    return (
      <p role="alert" className="text-xs text-destructive">
        {errorMessage(error)}
      </p>
    );
  }
  const own = orderAlerts(data.filter((a) => a.agent_id === agentId));
  if (own.length === 0) {
    return <p className="text-xs text-muted-foreground">Nothing about this agent needed a look.</p>;
  }
  return (
    <ul className="divide-y rounded-lg border" aria-label="Alerts">
      {own.map((alert) => (
        <AlertRow key={alert.id} alert={alert} workspaceId={workspaceId} acknowledge />
      ))}
    </ul>
  );
}

interface AgentCardSheetProps {
  /** The agent whose card is open; null closes the sheet. */
  agent: Agent | null;
  /** Whether the viewer may quarantine and release (workspace admins). */
  admin: boolean;
  onOpenChange: (open: boolean) => void;
  onEdit: (agent: Agent) => void;
}

/**
 * The agent's ID card, in tabs: who it is and what it may do (Overview; for an external agent
 * also its API tokens, issued and revoked there by admins), its last twenty actions
 * (Activity), the policy that governs it rule by rule (Policy), its held calls and alerts, its
 * A2A exposure with the card's addresses and the tasks it received (A2A), and the audit's
 * questions with the agent filled in (Ask). Admins quarantine and release from the footer.
 */
export function AgentCardSheet({ agent, admin, onOpenChange, onEdit }: AgentCardSheetProps) {
  const {
    data: card,
    error,
    isPending,
  } = useQuery({
    ...agentCardQuery(agent?.id ?? ''),
    enabled: agent !== null,
  });
  const quarantine = useQuarantineAgent();
  const workspaceId = useWorkspaceId();
  const shown = card?.agent ?? agent;
  const quarantined = shown?.status === 'quarantined';

  const toggle = () => {
    if (!shown) return;
    quarantine.mutate(
      { id: shown.id, release: quarantined },
      {
        onSuccess: (next) =>
          toast.success(
            next.status === 'quarantined'
              ? `${next.name} is quarantined: running nodes were cancelled`
              : `${next.name} is back in service`,
          ),
        onError: (err) => toast.error(errorMessage(err)),
      },
    );
  };

  return (
    <Sheet open={agent !== null} onOpenChange={onOpenChange}>
      <SheetContent className="flex w-full flex-col gap-0 overflow-y-auto sm:max-w-lg">
        {shown && (
          <>
            <SheetHeader className="gap-1">
              <SheetTitle className="flex flex-wrap items-center gap-2">
                {shown.name}
                <AgentStatusBadge status={shown.status} />
                {card?.evaluation && (
                  <Badge variant="outline" title="Competency questions passed">
                    {card.evaluation.score}/{card.evaluation.total}
                  </Badge>
                )}
              </SheetTitle>
              <SheetDescription className="flex items-center gap-1 font-mono text-xs">
                <span className="truncate">{shown.principal_id}</span>
                <CopyButton value={shown.principal_id} label="Copy principal" />
              </SheetDescription>
            </SheetHeader>
            <div className="flex-1 space-y-4 px-4 pb-4">
              {quarantined && (
                <p
                  role="status"
                  className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive"
                >
                  Quarantined {shown.quarantined_at ? formatDateTime(shown.quarantined_at) : ''}:
                  its running nodes were cancelled and new ones are refused until it is released.
                </p>
              )}
              {error && (
                <p role="alert" className="text-xs text-destructive">
                  {errorMessage(error)}
                </p>
              )}
              <Tabs defaultValue="overview">
                <TabsList
                  variant="line"
                  className="w-full overflow-x-auto"
                  aria-label="Card sections"
                >
                  <TabsTrigger value="overview">Overview</TabsTrigger>
                  <TabsTrigger value="activity">Activity</TabsTrigger>
                  <TabsTrigger value="policy">Policy</TabsTrigger>
                  <TabsTrigger value="approvals">Approvals</TabsTrigger>
                  <TabsTrigger value="alerts">Alerts</TabsTrigger>
                  <TabsTrigger value="a2a">A2A</TabsTrigger>
                  <TabsTrigger value="ask">Ask</TabsTrigger>
                </TabsList>
                <TabsContent value="overview">
                  <Overview shown={shown} card={card} isPending={isPending} admin={admin} />
                </TabsContent>
                <TabsContent value="activity">
                  {card ? <RecentSpans card={card} /> : <Skeleton className="h-16 rounded-lg" />}
                </TabsContent>
                <TabsContent value="policy">
                  {workspaceId ? (
                    <ResolvedPolicyView workspaceId={workspaceId} agentId={shown.id} />
                  ) : (
                    <Skeleton className="h-16 rounded-lg" />
                  )}
                </TabsContent>
                <TabsContent value="approvals">
                  {workspaceId && (
                    <AgentApprovals workspaceId={workspaceId} agentId={shown.id} admin={admin} />
                  )}
                </TabsContent>
                <TabsContent value="alerts">
                  {workspaceId && (
                    <AgentAlerts workspaceId={workspaceId} agentId={shown.id} admin={admin} />
                  )}
                </TabsContent>
                <TabsContent value="a2a">
                  <AgentA2a agent={shown} admin={admin} />
                </TabsContent>
                <TabsContent value="ask">
                  {workspaceId && (
                    <AskAuditPanel
                      key={shown.id}
                      workspaceId={workspaceId}
                      agentName={shown.name}
                      compact
                    />
                  )}
                </TabsContent>
              </Tabs>
            </div>
            <SheetFooter className="flex-row justify-between gap-2 border-t">
              <Button variant="outline" onClick={() => onEdit(shown)}>
                <PencilIcon />
                Edit
              </Button>
              {admin && (
                <AnimatedButton
                  variant={quarantined ? 'default' : 'destructive'}
                  loading={quarantine.isPending}
                  onClick={toggle}
                >
                  {quarantined ? <ShieldCheckIcon /> : <ShieldBanIcon />}
                  {quarantined ? 'Release' : 'Quarantine'}
                </AnimatedButton>
              )}
            </SheetFooter>
          </>
        )}
      </SheetContent>
    </Sheet>
  );
}
