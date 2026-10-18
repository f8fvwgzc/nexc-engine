import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { AppearanceCard } from '@/features/settings/components/appearance-card';

export default function PreferencesSettingsPage() {
  return (
    <PageContainer width="narrow">
      <Seo title="Preferences" noIndex />
      <PageHeader title="Preferences" description="How the app looks on this device." />
      <AppearanceCard />
    </PageContainer>
  );
}
