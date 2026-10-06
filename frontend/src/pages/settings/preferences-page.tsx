import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { AppearanceCard } from '@/features/settings/components/appearance-card';

export default function PreferencesSettingsPage() {
  return (
    <div className="mx-auto w-full max-w-3xl space-y-6 p-4 sm:p-6">
      <Seo title="Preferences" noIndex />
      <PageHeader title="Preferences" description="How the app looks on this device." />
      <AppearanceCard />
    </div>
  );
}
