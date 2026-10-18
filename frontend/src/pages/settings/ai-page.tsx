import { Suspense } from 'react';

import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { Skeleton } from '@/components/ui/skeleton';
import { LlmSettingsForm } from '@/features/settings/components/llm-settings-form';

export default function AiSettingsPage() {
  return (
    <PageContainer width="narrow">
      <Seo title="AI accounts" noIndex />
      <PageHeader
        title="AI accounts"
        description="Whose account pays for model calls: your own, then the workspace credential, then the server default."
      />
      <Suspense fallback={<Skeleton className="h-72 rounded-xl" />}>
        <LlmSettingsForm />
      </Suspense>
    </PageContainer>
  );
}
