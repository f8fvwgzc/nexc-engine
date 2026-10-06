import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';

import { CopyButton } from '@/components/custom-ui/copy-button';
import { GlassCard } from '@/components/custom-ui/glass-card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { updateProfile } from '@/features/account/api';
import { meQuery } from '@/features/auth/api';
import { errorMessage } from '@/lib/api/errors';
import { formatDateTime } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { useAuthStore } from '@/stores/auth-store';

const NAME_MAX = 100;

/** The name others see, changed in place. */
function NameField({ name }: { name: string }) {
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState(name);
  const save = useMutation({
    mutationFn: () => updateProfile(draft.trim()),
    meta: { successMessage: 'Name changed', errorToast: false },
    onSuccess: (user) => {
      useAuthStore.getState().setUser(user);
      queryClient.setQueryData(qk.me, user);
      setDraft(user.name);
      // Names are shown in member lists, issues and comments.
      void queryClient.invalidateQueries({ queryKey: ['workspaces'] });
    },
  });
  const changed = draft.trim() !== name && draft.trim().length > 0;
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (changed) save.mutate();
  };
  return (
    <form onSubmit={submit} className="space-y-1">
      <div className="flex max-w-sm gap-2">
        <Input
          aria-label="Name"
          value={draft}
          maxLength={NAME_MAX}
          autoComplete="name"
          className="h-8"
          onChange={(e) => setDraft(e.target.value)}
        />
        <Button type="submit" size="sm" disabled={!changed || save.isPending}>
          Save
        </Button>
      </div>
      {save.error ? (
        <p role="alert" className="text-xs text-destructive">
          {errorMessage(save.error)}
        </p>
      ) : null}
    </form>
  );
}

export function ProfileCard() {
  const sessionUser = useAuthStore((s) => s.user);
  const { data: user = sessionUser } = useQuery(meQuery());
  if (!user) return null;

  return (
    <GlassCard id="profile" className="scroll-mt-20 space-y-4 p-5 sm:p-6">
      <div className="flex items-center justify-between gap-2">
        <h2 className="font-medium">Profile</h2>
        <Badge variant="outline">{user.role === 'admin' ? 'Platform admin' : 'User'}</Badge>
      </div>
      <dl className="grid items-center gap-3 text-sm sm:grid-cols-[8rem_1fr]">
        <dt className="text-muted-foreground">Name</dt>
        <dd>
          <NameField key={user.name} name={user.name} />
        </dd>
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
