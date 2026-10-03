import { useSuspenseQuery } from '@tanstack/react-query';
import { BotIcon, PlusIcon } from 'lucide-react';
import { Suspense, useState } from 'react';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { agentsQuery } from '@/features/agents/api';
import { AgentFormDialog } from '@/features/agents/components/agent-form-dialog';
import { AgentOrgChart } from '@/features/agents/components/agent-org-chart';
import { AgentsTable } from '@/features/agents/components/agents-table';
import { useDeleteAgent } from '@/features/agents/hooks/use-agent-mutations';
import type { Agent } from '@/schemas/agent';

type Editing = { agent: Agent | null } | null;

function Agents() {
  const { data: agents } = useSuspenseQuery(agentsQuery());
  const [editing, setEditing] = useState<Editing>(null);
  const [deleting, setDeleting] = useState<Agent | null>(null);
  const deleteAgent = useDeleteAgent();
  const create = () => setEditing({ agent: null });

  return (
    <div className="mx-auto w-full max-w-6xl space-y-8 p-4 sm:p-6">
      <PageHeader
        title="Agents"
        description="Your AI org chart: who reports to whom, which roles execute which nodes, and how much budget each has left."
        actions={
          <Button onClick={create}>
            <PlusIcon />
            New agent
          </Button>
        }
      />
      {agents.length === 0 ? (
        <EmptyState
          icon={BotIcon}
          title="No agents yet"
          description="Create a researcher, a writer and a reviewer, then set node agent roles to match."
          action={<Button onClick={create}>Create your first agent</Button>}
        />
      ) : (
        <>
          <section aria-labelledby="org-chart" className="space-y-3">
            <h2 id="org-chart" className="text-sm font-medium">
              Org chart
            </h2>
            <AgentOrgChart agents={agents} onEdit={(agent) => setEditing({ agent })} />
          </section>
          <section aria-labelledby="all-agents" className="space-y-3">
            <h2 id="all-agents" className="text-sm font-medium">
              All agents
            </h2>
            <AgentsTable
              agents={agents}
              onEdit={(agent) => setEditing({ agent })}
              onDelete={setDeleting}
            />
          </section>
        </>
      )}
      <AgentFormDialog
        open={editing !== null}
        onOpenChange={(open) => !open && setEditing(null)}
        agent={editing?.agent ?? null}
        agents={agents}
      />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={`Delete ${deleting?.name ?? 'agent'}?`}
        description="The agent is removed permanently. Nodes that use its role will need another agent."
        confirmLabel="Delete agent"
        destructive
        onConfirm={() => deleting && deleteAgent.mutate(deleting.id)}
      />
    </div>
  );
}

export default function AgentsPage() {
  return (
    <>
      <Seo title="Agents" noIndex />
      <Suspense fallback={<PageSkeleton />}>
        <Agents />
      </Suspense>
    </>
  );
}
