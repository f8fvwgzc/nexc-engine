import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { ActivityCard } from '@/features/account/components/activity-card';
import { DataCard } from '@/features/account/components/data-card';
import { PasswordCard } from '@/features/account/components/password-card';
import { SessionsCard } from '@/features/account/components/sessions-card';
import { TwoFactorCard } from '@/features/account/components/two-factor-card';
import { ProfileCard } from '@/features/settings/components/profile-card';

/**
 * A person's own account: who they are here, how they sign in, where they are signed in, and
 * their data. The platform console shows the same page to its administrators.
 */
export default function ProfileSettingsPage() {
  return (
    <div className="mx-auto w-full max-w-3xl space-y-6 p-4 sm:p-6">
      <Seo title="Profile" noIndex />
      <PageHeader
        title="Profile"
        description="Your account: your name, your password, where you are signed in, what happened to your access, and your data."
      />
      <ProfileCard />
      <PasswordCard />
      <TwoFactorCard />
      <SessionsCard />
      <ActivityCard />
      <DataCard />
    </div>
  );
}
