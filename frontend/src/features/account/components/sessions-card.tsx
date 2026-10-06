import { useMutation, useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { GlassCard } from '@/components/custom-ui/glass-card';
import { Button } from '@/components/ui/button';
import { useAuthStore } from '@/stores/auth-store';

import { endSessions, sessionsQuery } from '../api';

/** How many sign-ins are alive, and a way to end them all. */
export function SessionsCard() {
  const navigate = useNavigate();
  const [asking, setAsking] = useState(false);
  const { data } = useQuery(sessionsQuery());
  const end = useMutation({
    mutationFn: endSessions,
    onSuccess: () => {
      useAuthStore.getState().clearSession();
      void navigate('/login', { replace: true });
    },
  });
  const others = data ? data.active - 1 : 0;
  return (
    <GlassCard id="sessions" className="scroll-mt-20 space-y-3 p-5 sm:p-6">
      <div>
        <h2 className="font-medium">Sessions</h2>
        <p className="text-sm text-muted-foreground">
          {!data
            ? 'Counting where you are signed in…'
            : others > 0
              ? `You are signed in here and on ${others} other ${others === 1 ? 'device or browser' : 'devices or browsers'}.`
              : 'You are signed in here only.'}
        </p>
      </div>
      <Button variant="outline" size="sm" disabled={end.isPending} onClick={() => setAsking(true)}>
        Sign out everywhere
      </Button>
      <ConfirmDialog
        open={asking}
        onOpenChange={setAsking}
        title="Sign out everywhere?"
        description="Every device and browser is signed out at once, this one included. You sign in again with your password."
        confirmLabel="Sign out everywhere"
        onConfirm={() => end.mutate()}
      />
    </GlassCard>
  );
}
