import { ChevronLeftIcon, ChevronRightIcon } from 'lucide-react';
import { useState, type ReactNode } from 'react';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useDebouncedValue } from '@/hooks/use-debounced-value';

import type { PlatformPage } from './api';

export const PLATFORM_PAGE_SIZE = 25;

/** The search box and page of a platform list. One more row than a page is asked for. */
export function usePlatformPage(placeholder = 'Search by name or e-mail…'): {
  page: PlatformPage;
  controls: (rows: number) => ReactNode;
  search: ReactNode;
} {
  const [q, setQ] = useState('');
  const [index, setIndex] = useState(0);
  const term = useDebouncedValue(q.trim(), 300);
  const [pagedFor, setPagedFor] = useState(term);
  if (pagedFor !== term) {
    setPagedFor(term);
    setIndex(0);
  }
  return {
    page: {
      q: term || undefined,
      limit: PLATFORM_PAGE_SIZE + 1,
      offset: index * PLATFORM_PAGE_SIZE,
    },
    search: (
      <Input
        type="search"
        value={q}
        placeholder={placeholder}
        aria-label="Search"
        className="h-8 max-w-xs text-[13px]"
        onChange={(e) => setQ(e.target.value)}
      />
    ),
    controls: (rows) =>
      (index > 0 || rows > PLATFORM_PAGE_SIZE) && (
        <nav
          aria-label="Pages"
          className="flex items-center justify-end gap-2 text-[13px] text-muted-foreground"
        >
          <span className="mr-1 tabular-nums">Page {index + 1}</span>
          <Button
            variant="outline"
            size="sm"
            disabled={index === 0}
            onClick={() => setIndex((i) => Math.max(0, i - 1))}
          >
            <ChevronLeftIcon />
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={rows <= PLATFORM_PAGE_SIZE}
            onClick={() => setIndex((i) => i + 1)}
          >
            Next
            <ChevronRightIcon />
          </Button>
        </nav>
      ),
  };
}
