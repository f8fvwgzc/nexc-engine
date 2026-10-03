import { useQuery } from '@tanstack/react-query';

import { CopyButton } from '@/components/custom-ui/copy-button';
import { GlassCard } from '@/components/custom-ui/glass-card';
import { Badge } from '@/components/ui/badge';
import { meQuery } from '@/features/auth/api';
import { formatDateTime } from '@/lib/format';
import { useAuthStore } from '@/stores/auth-store';

export function ProfileCard() {
  const sessionUser = useAuthStore((s) => s.user);
  const { data: user = sessionUser } = useQuery(meQuery());
  if (!user) return null;

  return (
    <GlassCard id="profile" className="scroll-mt-20 space-y-4 p-5 sm:p-6">
      <div className="flex items-center justify-between gap-2">
        <h2 className="font-medium">Profile</h2>
        <Badge variant="outline" className="capitalize">
          {user.role}
        </Badge>
      </div>
      <dl className="grid gap-3 text-sm sm:grid-cols-[8rem_1fr]">
        <dt className="text-muted-foreground">Name</dt>
        <dd>{user.name}</dd>
        <dt className="text-muted-foreground">Email</dt>
        <dd className="break-all">{user.email}</dd>
        <dt className="text-muted-foreground">Member since</dt>
        <dd>{formatDateTime(user.created_at)}</dd>
        <dt className="text-muted-foreground">User id</dt>
        <dd className="flex items-center gap-1 font-mono text-xs">
          <span className="truncate">{user.id}</span>
          <CopyButton value={user.id} label="Copy user id" />
        </dd>
      </dl>
    </GlassCard>
  );
}
