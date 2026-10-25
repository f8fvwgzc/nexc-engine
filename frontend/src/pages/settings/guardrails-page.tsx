import { Suspense } from 'react';

import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { SettingsCardsSkeleton } from '@/components/layout/page-skeleton';
import { QueryErrorBoundary } from '@/components/layout/query-error-boundary';
import { Seo } from '@/components/seo/seo';
import { GuardrailsCard } from '@/features/settings/components/guardrails-card';
import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';

export default function GuardrailsSettingsPage() {
  const workspaceId = useWorkspaceId();
  return (
    <PageContainer width="narrow">
      <Seo title="Guardrails" noIndex />
      <PageHeader
        title="Guardrails"
        description="Limits on what agents in this workspace may spend and do."
      />
      <QueryErrorBoundary key={workspaceId} title="Couldn’t load the guardrails">
        <Suspense fallback={<SettingsCardsSkeleton rows={4} />}>
          <GuardrailsCard />
        </Suspense>
      </QueryErrorBoundary>
    </PageContainer>
  );
}
