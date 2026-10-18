import { useRef, type ReactNode } from 'react';

import { useOverflow } from '@/hooks/use-overflow-x';
import { cn } from '@/lib/utils';

interface ScrollRegionProps {
  /** What the region holds, said when it takes focus ("Policy diff"). */
  label: string;
  as?: 'div' | 'pre' | 'p';
  className?: string;
  children: ReactNode;
}

/**
 * A box with a scrollbar of its own (a long diff, a payload, a capped list). While its content
 * overflows it is a named, focusable region, so the keyboard can scroll it too (WCAG 2.1.1); with
 * nothing to scroll it is a plain box and no extra tab stop.
 */
export function ScrollRegion({ label, as: Tag = 'div', className, children }: ScrollRegionProps) {
  const ref = useRef<HTMLElement | null>(null);
  const scrolls = useOverflow(ref, 'both');
  return (
    <Tag
      ref={(element: HTMLElement | null) => {
        ref.current = element;
      }}
      className={cn(
        'focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none',
        className,
      )}
      {...(scrolls ? { tabIndex: 0, role: 'group', 'aria-label': label } : {})}
    >
      {children}
    </Tag>
  );
}
