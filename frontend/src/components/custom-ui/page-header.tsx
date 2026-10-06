import type { ReactNode } from 'react';

import { cn } from '@/lib/utils';

import { FadeIn } from './motion';

interface PageHeaderProps {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  className?: string;
}

export function PageHeader({ title, description, actions, className }: PageHeaderProps) {
  return (
    <FadeIn
      className={cn(
        'flex flex-col gap-3 border-b pb-4 sm:flex-row sm:items-center sm:justify-between sm:gap-6',
        className,
      )}
    >
      <div className="min-w-0 space-y-1">
        <h1 className="text-lg font-semibold tracking-tight text-balance">{title}</h1>
        {description && (
          <p className="max-w-2xl text-[13px] text-pretty text-muted-foreground">{description}</p>
        )}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </FadeIn>
  );
}
