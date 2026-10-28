import type { ComponentProps } from 'react';

import { cn } from '@/lib/utils';

/**
 * The widths a page may take. Every page of an area uses the same one, so its title sits at the
 * same place from page to page.
 */
const WIDTH = {
  /** The app's pages: lists, boards and details, using all the room beside the sidebar. */
  default: 'max-w-none',
  /** Consoles that use every column they get (AgentSecOps, a session's tree and spans). */
  wide: 'max-w-none',
  /** Settings and the platform console: full width too, never a centred column. */
  narrow: 'max-w-none',
} as const;

export type PageWidth = keyof typeof WIDTH;

interface PageContainerProps extends ComponentProps<'div'> {
  width?: PageWidth;
}

/**
 * A page's frame: padded, as wide as its width allows, and its sections a steady 1.5 rem apart.
 * A page that holds a board (an element marked `data-page-fill`) becomes a column exactly as tall
 * as the space under the header, so the board takes the height left over and scrolls inside it.
 */
export function PageContainer({ width = 'default', className, ...props }: PageContainerProps) {
  return (
    <div
      data-slot="page"
      className={cn(
        'mx-auto w-full space-y-6 p-4 has-data-page-fill:flex has-data-page-fill:min-h-0 has-data-page-fill:flex-1 has-data-page-fill:flex-col sm:p-6',
        WIDTH[width],
        className,
      )}
      {...props}
    />
  );
}
