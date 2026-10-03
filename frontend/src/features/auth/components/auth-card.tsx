import type { ReactNode } from 'react';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { FadeIn } from '@/components/custom-ui/motion';

interface AuthCardProps {
  title: string;
  description: ReactNode;
  footer: ReactNode;
  children: ReactNode;
}

export function AuthCard({ title, description, footer, children }: AuthCardProps) {
  return (
    <FadeIn variant="scale" className="w-full max-w-sm">
      <GlassCard className="p-6 sm:p-8">
        <div className="mb-6 space-y-1.5 text-center">
          <h1 className="text-xl font-semibold tracking-tight">{title}</h1>
          <p className="text-sm text-muted-foreground">{description}</p>
        </div>
        {children}
      </GlassCard>
      <p className="mt-6 text-center text-sm text-muted-foreground">{footer}</p>
    </FadeIn>
  );
}
