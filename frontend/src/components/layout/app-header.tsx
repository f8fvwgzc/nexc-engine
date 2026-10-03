import { SearchIcon } from 'lucide-react';

import { KbdHint } from '@/components/custom-ui/kbd-hint';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { SidebarTrigger } from '@/components/ui/sidebar';
import { useCommandStore } from '@/stores/command-store';

import { Breadcrumbs } from './breadcrumbs';
import { ThemeToggle } from './theme-toggle';

export function AppHeader() {
  const openPalette = useCommandStore((s) => s.setPaletteOpen);
  return (
    <header className="sticky top-0 z-20 flex h-14 shrink-0 items-center gap-2 border-b bg-background/80 px-3 backdrop-blur-lg transition-[height] ease-linear group-has-data-[collapsible=icon]/sidebar-wrapper:h-12 sm:px-4">
      <SidebarTrigger className="-ml-1" aria-label="Toggle sidebar" />
      <Separator orientation="vertical" className="mr-1 data-[orientation=vertical]:h-4" />
      <Breadcrumbs />
      <div className="ml-auto flex items-center gap-1">
        <Button
          variant="outline"
          size="sm"
          onClick={() => openPalette(true)}
          className="gap-2 text-muted-foreground sm:w-52 sm:justify-between"
          aria-label="Open command palette"
        >
          <span className="flex items-center gap-2">
            <SearchIcon className="size-3.5" />
            <span className="hidden sm:inline">Search or run…</span>
          </span>
          <KbdHint keys="mod+k" />
        </Button>
        <ThemeToggle />
      </div>
    </header>
  );
}
