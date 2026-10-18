import { SearchIcon, SparklesIcon } from 'lucide-react';

import { IconButton } from '@/components/custom-ui/icon-button';
import { KbdHint } from '@/components/custom-ui/kbd-hint';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { SidebarTrigger } from '@/components/ui/sidebar';
import { useAssistantStore } from '@/stores/assistant-store';
import { useCommandStore } from '@/stores/command-store';

import { Breadcrumbs } from './breadcrumbs';
import { ThemeToggle } from './theme-toggle';

export function AppHeader() {
  const openPalette = useCommandStore((s) => s.setPaletteOpen);
  const openAssistant = useAssistantStore((s) => s.setOpen);
  return (
    <header className="sticky top-0 z-20 flex h-11 shrink-0 items-center gap-2 border-b bg-background/80 px-3 backdrop-blur-lg sm:px-4">
      {/* Only a phone needs a way to open the sidebar; on desktop it is always there. */}
      <SidebarTrigger className="-ml-1 md:hidden" aria-label="Open navigation" />
      <Separator
        orientation="vertical"
        className="mr-1 data-[orientation=vertical]:h-4 md:hidden"
      />
      <Breadcrumbs />
      <div className="ml-auto flex items-center gap-1">
        {/* A phone has room for the icon only: the tooltip and the name say what it opens. */}
        <IconButton
          variant="outline"
          size="icon-sm"
          label="Search or run a command"
          onClick={() => openPalette(true)}
          className="text-muted-foreground sm:hidden"
        >
          <SearchIcon />
        </IconButton>
        <Button
          variant="outline"
          size="sm"
          onClick={() => openPalette(true)}
          className="hidden w-52 justify-between gap-2 text-muted-foreground sm:flex"
          aria-label="Open command palette"
        >
          <span className="flex items-center gap-2">
            <SearchIcon className="size-3.5" />
            Search or run…
          </span>
          <KbdHint keys="mod+k" />
        </Button>
        {/* On a phone the assistant opens from here: its notch would sit on the page. */}
        <IconButton
          label="Ask the assistant"
          className="md:hidden"
          onClick={() => openAssistant(true)}
        >
          <SparklesIcon />
        </IconButton>
        <ThemeToggle />
      </div>
    </header>
  );
}
