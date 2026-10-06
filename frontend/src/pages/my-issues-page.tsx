import { useState } from 'react';

import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { useAuthStore } from '@/stores/auth-store';

import { IssueExplorer } from './issues-page';

const TABS = [
  { key: 'assigned', label: 'Assigned to me' },
  { key: 'created', label: 'Created by me' },
] as const;
type Tab = (typeof TABS)[number]['key'];

/**
 * One person's issues across every team of the workspace they can see: what is theirs to do, and
 * what they filed for others. The same list, board and filters as the Issues page.
 */
export default function MyIssuesPage() {
  const { current } = useCurrentWorkspace();
  const me = useAuthStore((s) => s.user?.id);
  const [tab, setTab] = useState<Tab>('assigned');
  return (
    <div className="mx-auto w-full max-w-5xl space-y-5 p-4 sm:p-6">
      <Seo title="My issues" noIndex />
      <PageHeader
        title="My issues"
        description={
          current
            ? `What is yours to do in ${current.name}, and what you filed, across its teams.`
            : 'What is yours to do, and what you filed.'
        }
      />
      <div role="tablist" aria-label="Which of your issues" className="flex gap-1">
        {TABS.map((option) => (
          <Button
            key={option.key}
            role="tab"
            aria-selected={option.key === tab}
            variant={option.key === tab ? 'secondary' : 'ghost'}
            size="sm"
            onClick={() => setTab(option.key)}
          >
            {option.label}
          </Button>
        ))}
      </div>
      {current && me ? (
        <IssueExplorer
          key={`${current.id}:${tab}`}
          workspace={current}
          person={tab === 'assigned' ? { assignee_id: me } : { creator_id: me }}
        />
      ) : (
        <PageSkeleton />
      )}
    </div>
  );
}
