import type { ComponentProps } from 'react';

import { cn } from '@/lib/utils';

export function GradientText({ className, ...props }: ComponentProps<'span'>) {
  return (
    <span
      className={cn(
        'bg-gradient-to-r from-brand via-[color-mix(in_oklch,var(--brand),var(--brand-2))] to-brand-2 bg-clip-text text-transparent',
        className,
      )}
      {...props}
    />
  );
}
