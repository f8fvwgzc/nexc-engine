import { Suspense } from 'react';

import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { Skeleton } from '@/components/ui/skeleton';
import { GuardrailsCard } from '@/features/settings/components/guardrails-card';

export default function GuardrailsSettingsPage() {
  return (
    <div className="mx-auto w-full max-w-3xl space-y-6 p-4 sm:p-6">
      <Seo title="Guardrails" noIndex />
      <PageHeader
        title="Guardrails"
        description="Limits on what agents in this workspace may spend and do."
      />
      <Suspense fallback={<Skeleton className="h-72 rounded-xl" />}>
        <GuardrailsCard />
      </Suspense>
    </div>
  );
}
