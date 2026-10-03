import { Suspense } from 'react';

import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { Skeleton } from '@/components/ui/skeleton';
import { AppearanceCard } from '@/features/settings/components/appearance-card';
import { LlmSettingsForm } from '@/features/settings/components/llm-settings-form';
import { ProfileCard } from '@/features/settings/components/profile-card';

export default function SettingsPage() {
  return (
    <div className="mx-auto w-full max-w-3xl space-y-6 p-4 sm:p-6">
      <Seo title="Settings" noIndex />
      <PageHeader title="Settings" description="Model provider, API key, appearance and profile." />
      <Suspense fallback={<Skeleton className="h-96 rounded-xl" />}>
        <LlmSettingsForm />
      </Suspense>
      <AppearanceCard />
      <ProfileCard />
    </div>
  );
}
