import { Suspense } from 'react';

import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { Skeleton } from '@/components/ui/skeleton';
import { LlmSettingsForm } from '@/features/settings/components/llm-settings-form';

export default function AiSettingsPage() {
  return (
    <div className="mx-auto w-full max-w-3xl space-y-6 p-4 sm:p-6">
      <Seo title="AI accounts" noIndex />
      <PageHeader
        title="AI accounts"
        description="Whose account pays for model calls: your own, then the workspace credential, then the server default."
      />
      <Suspense fallback={<Skeleton className="h-72 rounded-xl" />}>
        <LlmSettingsForm />
      </Suspense>
    </div>
  );
}
