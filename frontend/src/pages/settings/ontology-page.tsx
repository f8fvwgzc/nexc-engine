import { useQuery } from '@tanstack/react-query';
import { BotIcon, GitPullRequestArrowIcon, ShapesIcon, WaypointsIcon } from 'lucide-react';
import { Link } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { ContentSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { ontologyStatusQuery } from '@/features/ontology/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { formatDateTime, formatRelative } from '@/lib/format';
import type { SchemaProposal } from '@/schemas/ontology';
import type { Workspace } from '@/schemas/workspace';

const MAP = '/app/settings/map';

/** Opens a view of onto-intelli's UI inside the console, signed in as the reader. */
const inConsole = (view: string) => `${MAP}?view=${encodeURIComponent(view)}`;

function Proposal({ proposal }: { proposal: SchemaProposal }) {
  return (
    <li className="flex flex-col gap-2 px-3 py-2.5 text-[13px] sm:flex-row sm:items-start sm:gap-3">
      <div className="min-w-0 flex-1 space-y-0.5">
        <p className="flex flex-wrap items-center gap-2 font-medium">
          {proposal.title}
          {proposal.breaking && <Badge variant="destructive">Breaking</Badge>}
        </p>
        {proposal.rationale && (
          <p className="text-xs text-muted-foreground">{proposal.rationale}</p>
        )}
        <p className="flex flex-wrap items-center gap-x-2 text-xs text-muted-foreground">
          {proposal.by_agent && (
            <span className="inline-flex items-center gap-1">
              <BotIcon className="size-3.5" aria-hidden />
              the assistant
            </span>
          )}
          <span className="font-mono">{proposal.proposer}</span>
          <span>
            {proposal.changes} change{proposal.changes === 1 ? '' : 's'}
          </span>
          <time dateTime={proposal.created_at} title={formatDateTime(proposal.created_at)}>
            {formatRelative(proposal.created_at)}
          </time>
        </p>
      </div>
      <Button variant="outline" size="sm" asChild className="shrink-0">
        <Link to={inConsole(`/proposals/${proposal.id}`)}>
          <GitPullRequestArrowIcon />
          Review
        </Link>
      </Button>
    </li>
  );
}

function Ontology({ workspace }: { workspace: Workspace }) {
  const { data: status, isPending, error, refetch } = useQuery(ontologyStatusQuery(workspace.id));
  if (isPending) return <Skeleton className="h-48 rounded-xl" />;
  if (error) {
    return (
      <ErrorState title="Couldn’t load the ontology" error={error} onRetry={() => void refetch()} />
    );
  }
  if (status.mode === 'off') {
    return (
      <EmptyState
        icon={ShapesIcon}
        title="onto-intelli is not configured"
        description="Entity kinds, link types, actions and every graph’s node and relation types live in onto-intelli. Without it, graphs use the read-only starter vocabulary and nothing can be proposed."
        action={
          <Button variant="outline" asChild>
            <Link to={MAP}>
              <WaypointsIcon />
              What the map needs
            </Link>
          </Button>
        }
      />
    );
  }
  return (
    <div className="space-y-6">
      <section aria-label="Where it lives" className="space-y-2">
        <h2 className="text-[13px] font-medium">Where it lives</h2>
        <dl className="grid gap-x-4 gap-y-1 rounded-lg border px-3 py-2.5 text-[13px] sm:grid-cols-[10rem_1fr]">
          <dt className="text-muted-foreground">onto-intelli tenant</dt>
          <dd className="font-mono break-all">{status.tenant}</dd>
          <dt className="text-muted-foreground">Workspace there</dt>
          <dd className="font-mono break-all">{status.workspace}</dd>
        </dl>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" asChild>
            <Link to={inConsole('/schema')}>
              <ShapesIcon />
              Edit the schema
            </Link>
          </Button>
          <Button variant="outline" size="sm" asChild>
            <Link to={inConsole('/proposals')}>
              <GitPullRequestArrowIcon />
              All proposals
            </Link>
          </Button>
        </div>
      </section>
      <section aria-label="Open proposals" className="space-y-2">
        <h2 className="text-[13px] font-medium">Open proposals</h2>
        {status.error && (
          <p role="status" className="text-[13px] text-muted-foreground">
            onto-intelli did not answer: {status.error}
          </p>
        )}
        {status.proposals.length === 0 ? (
          <EmptyState
            icon={GitPullRequestArrowIcon}
            title="Nothing to review"
            description="Changes to entity kinds, link types and actions that someone, or the assistant, proposes wait here until an approver decides in onto-intelli."
            action={
              <Button variant="outline" asChild>
                <Link to={inConsole('/schema')}>
                  <ShapesIcon />
                  Propose a change
                </Link>
              </Button>
            }
          />
        ) : (
          <ol className="divide-y rounded-lg border">
            {status.proposals.map((proposal) => (
              <Proposal key={proposal.id} proposal={proposal} />
            ))}
          </ol>
        )}
      </section>
    </div>
  );
}

export default function OntologySettingsPage() {
  const { current } = useCurrentWorkspace();
  return (
    <PageContainer width="narrow">
      <Seo title="Ontology" noIndex />
      <PageHeader
        title="Ontology"
        description="What the workspace models, kept in onto-intelli: entity kinds, link types, actions and the vocabulary of every graph. Approvers decide on proposed changes there; the assistant may propose, never apply."
      />
      {!current ? (
        <ContentSkeleton />
      ) : current.role === 'guest' ? (
        <EmptyState
          icon={ShapesIcon}
          title="Guests do not see the ontology"
          description="Ask an owner or admin for a member’s role."
        />
      ) : (
        <Ontology key={current.id} workspace={current} />
      )}
    </PageContainer>
  );
}
