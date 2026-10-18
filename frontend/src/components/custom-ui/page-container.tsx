import type { ComponentProps } from 'react';

import { cn } from '@/lib/utils';

/**
 * The widths a page may take. Every page of an area uses the same one, so its title sits at the
 * same place from page to page.
 */
const WIDTH = {
  /** The app's pages: lists, boards and details. */
  default: 'max-w-6xl',
  /** Consoles that use every column they get (AgentSecOps, a session's tree and spans). */
  wide: 'max-w-[96rem]',
  /** Settings and the platform console. */
  narrow: 'max-w-5xl',
} as const;

export type PageWidth = keyof typeof WIDTH;

interface PageContainerProps extends ComponentProps<'div'> {
  width?: PageWidth;
}

/** A page's frame: centred, padded, and its sections a steady 1.5 rem apart. */
export function PageContainer({ width = 'default', className, ...props }: PageContainerProps) {
  return (
    <div
      data-slot="page"
      className={cn('mx-auto w-full space-y-6 p-4 sm:p-6', WIDTH[width], className)}
      {...props}
    />
  );
}
