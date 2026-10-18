import { Suspense } from 'react';

import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { Skeleton } from '@/components/ui/skeleton';
import { GuardrailsCard } from '@/features/settings/components/guardrails-card';

export default function GuardrailsSettingsPage() {
  return (
    <PageContainer width="narrow">
      <Seo title="Guardrails" noIndex />
      <PageHeader
        title="Guardrails"
        description="Limits on what agents in this workspace may spend and do."
      />
      <Suspense fallback={<Skeleton className="h-72 rounded-xl" />}>
        <GuardrailsCard />
      </Suspense>
    </PageContainer>
  );
}
