import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { ProfileCard } from '@/features/settings/components/profile-card';

export default function ProfileSettingsPage() {
  return (
    <div className="mx-auto w-full max-w-3xl space-y-6 p-4 sm:p-6">
      <Seo title="Profile" noIndex />
      <PageHeader title="Profile" description="Your account in this app." />
      <ProfileCard />
    </div>
  );
}
