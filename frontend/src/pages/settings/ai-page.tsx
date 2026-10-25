import { Suspense } from 'react';

import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { SettingsCardsSkeleton } from '@/components/layout/page-skeleton';
import { QueryErrorBoundary } from '@/components/layout/query-error-boundary';
import { Seo } from '@/components/seo/seo';
import { LlmSettingsForm } from '@/features/settings/components/llm-settings-form';
import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';

export default function AiSettingsPage() {
  const workspaceId = useWorkspaceId();
  return (
    <PageContainer width="narrow">
      <Seo title="AI accounts" noIndex />
      <PageHeader
        title="AI accounts"
        description="Whose account pays for model calls: your own, then the workspace credential, then the server default."
      />
      <QueryErrorBoundary key={workspaceId} title="Couldn’t load the AI accounts">
        <Suspense fallback={<SettingsCardsSkeleton rows={4} />}>
          <LlmSettingsForm />
        </Suspense>
      </QueryErrorBoundary>
    </PageContainer>
  );
}
