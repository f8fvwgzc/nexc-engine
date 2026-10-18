import { useQuery } from '@tanstack/react-query';
import { ShieldCheckIcon } from 'lucide-react';
import { useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { ContentSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { agentsQuery } from '@/features/agents/api';
import { AgentCardSheet } from '@/features/agents/components/agent-card-sheet';
import { AskAuditPanel } from '@/features/audit/components/ask-audit-panel';
import { secopsOverviewQuery } from '@/features/secops/api';
import { AlertsPanel } from '@/features/secops/components/alerts-panel';
import { ApprovalsPanel } from '@/features/secops/components/approvals-panel';
import { Panel } from '@/features/secops/components/console-panel';
import { LiveFeedPanel } from '@/features/secops/components/live-feed-panel';
import { PoliciesPanel } from '@/features/secops/components/policies-panel';
import { RegistryPanel } from '@/features/secops/components/registry-panel';
import { RiskPanel } from '@/features/secops/components/risk-panel';
import { SecopsStats } from '@/features/secops/components/secops-stats';
import { TraceExplorer } from '@/features/secops/components/trace-explorer';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { formatRelative } from '@/lib/format';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';
import { ScrollTextIcon } from 'lucide-react';

function Console({ workspace }: { workspace: Workspace }) {
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const tab = params.get('tab') === 'explorer' ? 'explorer' : 'console';
  const admin = isWorkspaceAdmin(workspace.role);
  const member = workspace.role !== 'guest';
  const overview = useQuery(secopsOverviewQuery(workspace.id));
  const { data: agents = [] } = useQuery(agentsQuery(workspace.id));
  // `?agent=` opens a card, as the Agents page does; the risk panel and the audit link here.
  const [cardId, setCardId] = useState<string | null>(() => params.get('agent'));
  const card = agents.find((a) => a.id === cardId) ?? null;
  const openCard = (id: string | null) => {
    setCardId(id);
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        if (id) next.set('agent', id);
        else next.delete('agent');
        return next;
      },
      { replace: true },
    );
  };
  return (
    <>
      <Tabs
        value={tab}
        onValueChange={(value) =>
          setParams(
            (prev) => {
              const next = new URLSearchParams(prev);
              if (value === 'explorer') next.set('tab', 'explorer');
              else next.delete('tab');
              return next;
            },
            { replace: true },
          )
        }
      >
        <TabsList variant="line" aria-label="Console views">
          <TabsTrigger value="console">Console</TabsTrigger>
          <TabsTrigger value="explorer">Trace explorer</TabsTrigger>
        </TabsList>
        <TabsContent value="console" className="space-y-4">
          {overview.error && !overview.data ? (
            <ErrorState
              compact
              title="Couldn’t load the overview"
              error={overview.error}
              onRetry={() => void overview.refetch()}
            />
          ) : (
            <div className="@container">
              <SecopsStats overview={overview.data} />
            </div>
          )}
          <div className="grid gap-4 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
            <div className="min-w-0 space-y-4">
              <RegistryPanel
                workspaceId={workspace.id}
                agents={agents}
                admin={admin}
                member={member}
                onOpen={(agent) => openCard(agent.id)}
              />
              <PoliciesPanel
                workspaceId={workspace.id}
                agents={agents}
                overview={overview.data}
                admin={admin}
              />
              {member && (
                <Panel id="secops-ask" title="Ask the audit" icon={ScrollTextIcon}>
                  <AskAuditPanel workspaceId={workspace.id} compact />
                </Panel>
              )}
            </div>
            <div className="min-w-0 space-y-4">
              <LiveFeedPanel workspaceId={workspace.id} agents={agents} />
              <ApprovalsPanel workspaceId={workspace.id} admin={admin} />
              <AlertsPanel workspaceId={workspace.id} admin={admin} />
              <RiskPanel
                overview={overview.data}
                error={overview.error}
                onRetry={() => void overview.refetch()}
                agents={agents}
                onOpenAgent={openCard}
              />
            </div>
          </div>
          {overview.data && (
            <p className="text-right text-[11px] text-muted-foreground">
              Overview as of {formatRelative(overview.data.at)}; refreshed every 15 seconds.
            </p>
          )}
        </TabsContent>
        <TabsContent value="explorer">
          <TraceExplorer workspaceId={workspace.id} agents={agents} />
        </TabsContent>
      </Tabs>
      <AgentCardSheet
        agent={card}
        admin={admin}
        onOpenChange={(open) => !open && openCard(null)}
        onEdit={(agent) => navigate(`/app/agents?agent=${agent.id}`)}
      />
    </>
  );
}

export default function SecopsPage() {
  const { current } = useCurrentWorkspace();
  return (
    <PageContainer width="wide">
      <Seo title="AgentSecOps" noIndex />
      <PageHeader
        title="AgentSecOps"
        description={
          current
            ? `Every agent of ${current.name}, what it did, what was refused, who approved what and how far each strays from its usual self: one screen, live.`
            : 'The security console of the workspace.'
        }
      />
      {!current ? (
        <ContentSkeleton />
      ) : current.role === 'guest' ? (
        <EmptyState
          icon={ShieldCheckIcon}
          title="Members see the console"
          description="Ask an owner or admin of this workspace to make you a member."
        />
      ) : (
        <Console key={current.id} workspace={current} />
      )}
    </PageContainer>
  );
}
